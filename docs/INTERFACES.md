# 排谷系统 · 模块接口与数据契约（INTERFACES）

> 对应 [REQUIREMENTS.md](./REQUIREMENTS.md) §5（C3）与 [GAP-ANALYSIS.md](./GAP-ANALYSIS.md) A0/T7。
> 事实来源为源码阅读，引用 `文件:行号`；未实现或不确定处显式标注「待 Tn / 待确认」。本文件为 T7 独占产物，不改动任何源码。
> 解耦目标：`web → api → pipeline → engine / settlement / messages`，禁止反向依赖或跨层直读内部状态。

---

> # ⚠️ 快照告示（务必先读）
>
> **以下 §1–§6 为 Wave 1–4 之前的快照**（T7 产物，2026-09-14），其中大量「尚未实现 / 待 Tn / 不存在 / 501」结论**已过时**。
> 旧栈（`repo/services/ws/publisher/config/app_state/inbound`、旧 `api` 路由、`migrations/**`）**已在 C-4 删除**。
> **现以实现与 [§8 Wave 1-4 新增/变更接口](#8-wave-1-4-新增变更接口2026-09-17) 为准**；阅读 §1–§6 时请以本节告示为准、勿据其旧结论判断现状。

---

## 1. 模块清单

| 模块 | 一句话职责 | 文件路径 |
|---|---|---|
| `gateway` | 反向 WS 服务器：接 NapCat 事件，按群白名单路由/Drop，只读动作回包（默认禁 `send_*`）。 | `src/gateway/{mod.rs,onebot.rs,ws_server.rs,action.rs}` |
| `llm::pipeline` | 单条消息流水线：幂等 → 规则快路径 → LLM 抽取 → 校验 → 权限 → 写事件 → 重建快照。 | `src/llm/{mod.rs,client.rs,prompt.rs,pipeline.rs}` |
| `engine::allocation_engine` | 确定性分配引擎：按优先级把生效认领排入盒槽/单领/等待，产出 `AllocationSnapshot`。 | `src/engine/allocation_engine.rs` |
| `engine::replay` | 内存事件重放服务：排序事件 → 收集生效 claim → 分配；同步重放唯一入口。 | `src/engine/replay.rs` |
| `replay::replay_engine` | 逐步重放引擎：对每条事件产出 `ReplayStep`（含 diff、决策轨迹）。 | `src/replay/replay_engine.rs` |
| `replay::state_diff` | 前后 `AllocationSnapshot` 的结构化差异（槽/认领/用户/商品/结算）。 | `src/replay/state_diff.rs` |
| `settlement` | 结算引擎 v2（调价/折扣/范围/特典档位/减均），**已实现**（Wave 1–4）。 | `src/settlement/{mod,engine,model}.rs` |
| `messages` | 消息日志 `MessageLog`（JSONL 追加/读取/替换 + 细粒度 query/update/delete + 原始事件），**已实现**。 | `src/messages/mod.rs` |
| `round` | 拼团阶段模型：时间窗判定 + 阶段权限矩阵（可排/可撤）。 | `src/round/mod.rs` |
| `snapshot_bundle` | 单一 JSON 快照 `SnapshotFile` + 导出/导入，**已实现**（Wave 2/C-2）。 | `src/snapshot_bundle/mod.rs` |
| `api` | axum HTTP 路由 + 静态页服务（config/board/display/messages/replay/settlement/sim/members）。 | `src/api/{mod.rs,*_routes.rs}` |
| `web` | vanilla JS 静态前端（display/admin/sim/replay），只经 HTTP API 取数。 | `web/**` |
| `scripts` | 脚本化模拟/录制重放（`--speed` 倍率、`record.jsonl`）。 | `scripts/{sim-lib,sim-run,sim-record}.mjs`（**已创建**，Wave 1–4；另含 `privacy-scan.mjs`） |
| `tests` | Node Playwright e2e + Rust 重放辅助测试。 | `tests/e2e/sim.mjs`、`src/tests/replay_helpers.rs` |

> 配套（非本表但被引用）：`src/bus.rs`（冻结 DTO/trait）、`src/settings.rs`（`AppConfig`+`ConfigStore`）、`src/parser/**`（规则/校验）、`src/domain/**`（事件/快照/结算类型）、`src/engine/settlement_engine.rs`（既有结算引擎）、`src/engine/event_store.rs`（`EventStore`）。

---

## 2. 接口表

### 2.1 Rust 公开接口

| 模块 | 名称 | 签名 | 输入 DTO | 输出 DTO | 错误 | 调用方 |
|---|---|---|---|---|---|---|
| `bus` | `IncomingEvent` | struct | — | — | — | Gateway→Pipeline |
| `bus` | `EventSink::handle` | `async fn handle(&self, ev: IncomingEvent)` | `IncomingEvent` | `()` | — | `Gateway::on_text`（`src/gateway/ws_server.rs:274`） |
| `bus` | `PipelineOutcome` | struct（`Serialize`） | — | — | — | `Pipeline::process` |
| `gateway` | `Gateway::new` | `fn new(cfg: Arc<ConfigStore>, sink: Arc<dyn EventSink>) -> Arc<Self>` | `AppConfig`、`EventSink` | `Arc<Gateway>` | — | `main.rs:96` |
| `gateway` | `Gateway::run` | `async fn run(self: Arc<Self>) -> anyhow::Result<()>` | — | `()` | `anyhow`（bind 失败循环重试） | `main.rs:101` |
| `gateway` | `Gateway::send_action` | `async fn send_action(&self, action: &str, params: Value) -> anyhow::Result<Value>` | action 名、params | OneBot 响应 `Value` | `forbidden send action`/`not allowed`/`no connected client`/timeout（`ws_server.rs:75-116`） | `member_routes::refresh_members_from_gateway` |
| `gateway` | `Gateway::status` | `async fn status(&self) -> Value` | — | `{listening,bound_addr,clients,last_error}` | — | `board_routes`、`display_routes` |
| `gateway::onebot` | `decide_route` | `fn decide_route(&RouteMessageEvent, &RoutePolicy) -> RouteKind` | `RouteMessageEvent`、`RoutePolicy` | `RouteKind{Drop,Message}` | — | `Gateway::on_text`（`ws_server.rs:262`） |
| `gateway::onebot` | `normalize_message` | `fn normalize_message(&RouteMessageEvent) -> String` | `RouteMessageEvent` | 纯文本 | — | `decide_route`、`to_incoming_event` |
| `gateway::onebot` | `clean_nickname` | `fn clean_nickname(&str) -> (String,String)` | 原始昵称 | `(identity, display)` | — | `parse_identity`、`Pipeline::process`（单一真源） |
| `gateway::onebot` | `parse_identity` | `fn parse_identity(&RouteMessageEvent) -> Identity` | `RouteMessageEvent` | `Identity` | — | `to_incoming_event` |
| `gateway::onebot` | `to_incoming_event` | `fn to_incoming_event(&RouteMessageEvent) -> IncomingEvent` | `RouteMessageEvent` | `IncomingEvent` | — | `Gateway::on_text`（`ws_server.rs:271`） |
| `gateway::onebot` | `sanitize` / `strip_cq_codes` / `normalize_fullwidth` / `group_id_string` / `value_to_string` | 工具函数（`onebot.rs:62-163`） | 字符串/`Value` | 字符串 | — | 日志与规范化 |
| `gateway::action` | `get_group_member_list` 等只读封装 | `async fn ...(gw,&str) -> anyhow::Result<Value>` | group_id | `Value` | 无客户端 | **仅 `#[cfg(test)]`**（`action.rs:1`），非生产路径 |
| `llm` | `truncate` | `pub(crate) fn truncate(text:&str, max:usize) -> String` | 文本 | 截断文本 | — | 日志/错误信息（`llm/mod.rs:8`） |
| `llm::client` | `LlmClient::complete` | `async fn complete(&self, settings:&LlmSettings, system_prompt:&str, user_prompt:&str) -> anyhow::Result<String>` | `LlmSettings`、提示词 | LLM 原始 JSON 串 | `anyhow`（超时/HTTP/JSON） | `Pipeline::llm_parse`（`pipeline.rs:408`） |
| `llm::client` | `OpenAiClient::new` | `fn new() -> Self` | — | `OpenAiClient` | — | `Pipeline::new`（`pipeline.rs:58`） |
| `llm::prompt` | `build_system_prompt` / `build_user_prompt` | `fn(cfg:&AppConfig)->String` / `fn(ev:&IncomingEvent)->String` | `AppConfig`/`IncomingEvent` | prompt | — | `Pipeline::llm_parse` |
| `llm::pipeline` | `Pipeline::new` | `fn new(cfg: Arc<ConfigStore>) -> Arc<Self>` | `ConfigStore` | `Arc<Pipeline>` | — | `main.rs:95` |
| `llm::pipeline` | `Pipeline::new_with_client` | `fn new_with_client(cfg, llm: Arc<dyn LlmClient>) -> Arc<Self>` | `ConfigStore`、`LlmClient` | `Arc<Pipeline>` | — | 单测（`pipeline.rs:706`） |
| `llm::pipeline` | `Pipeline::process` | `async fn process(&self, ev: IncomingEvent) -> PipelineOutcome` | `IncomingEvent` | `PipelineOutcome` | 内部转 `status=Error` | `sim_routes::sim_message`、`EventSink::handle` |
| `llm::pipeline` | `Pipeline::reset` | `async fn reset(&self)` | — | `()` | — | `sim_routes::sim_reset` |
| `llm::pipeline` | `Pipeline::board` | `async fn board(&self) -> (i64, Value)` | — | `(version, snapshot)` | — | `board_routes`、`display_routes` |
| `llm::pipeline` | `Pipeline::messages_since` | `async fn messages_since(&self, since:i64) -> Vec<Value>` | `since` | 消息数组 | — | `display_routes` |
| `llm::pipeline` | `Pipeline::who_whats` | `async fn who_whats(&self) -> Vec<Value>` | — | who-whats 数组 | — | `display_routes` |
| `engine::allocation_engine` | `AllocationEngine::new` / `allocate` | `fn new()->Self`；`fn allocate(&self, items:&[Item], claim_lines:&[EffectiveClaimLine], events:&[EventEnvelope]) -> AppResult<AllocationSnapshot>` | `Item`、`EffectiveClaimLine`、事件 | `AllocationSnapshot` | `AppResult` | `ReplayService`、`ReplayRuntimeState`、`rebuild_allocation_snapshot` |
| `engine::replay` | `ReplayService::new` | `fn new(event_store: Arc<dyn EventStore>) -> Self` | `EventStore` | `ReplayService` | — | 重放/测试 |
| `engine::replay` | `ReplayService::rebuild_snapshot` | `async fn rebuild_snapshot(&self, round_id:&RoundId, items:&[Item], eligibilities:&[Eligibility], _round:&Round) -> AppResult<(AllocationSnapshot, Option<SettlementSnapshot>)>` | 轮次/商品/资格 | 分配+可选结算快照 | `AppResult` | 旧栈/测试（`engine/replay.rs:33`） |
| `engine::replay` | `ReplayService::collect_effective_claims` | `fn collect_effective_claims(&self, events:&[EventEnvelope], eligibilities:&[Eligibility]) -> Vec<EffectiveClaimLine>` | 事件/资格 | 生效认领行 | — | `rebuild_allocation_snapshot`、`rebuild_snapshot` |
| `engine::replay` | `rebuild_allocation_snapshot` | `fn rebuild_allocation_snapshot(items:&[Item], events:&[EventEnvelope], eligibilities:&[Eligibility]) -> AllocationSnapshot` | 商品/事件/资格 | `AllocationSnapshot` | —（失败回空快照） | `Pipeline::process`（`pipeline.rs:288`） |
| `engine::replay` | `describe_event` | `fn describe_event(event:&EventEnvelope) -> String` | 事件 | 摘要串 | — | `Pipeline::process`（`pipeline.rs:278`） |
| `engine::event_store` | `EventStore::{append,read_all}` / `InMemoryEventStore::new` | `async fn append(&self,&EventEnvelope)->AppResult<EventEnvelope>`；`async fn read_all(&self,&RoundId)->AppResult<Vec<EventEnvelope>>` | 事件/轮次 | 事件 | `AppResult` | `ReplayService` |
| `engine::settlement_engine` | `SettlementEngine::settle` | `fn settle(&self, input:&SettlementInput) -> Result<SettlementSnapshot, SettlementError>` | `SettlementInput{allocation,items,discount_rules}` | `SettlementSnapshot` | `SettlementError`（空枚举） | `ReplayService`、`ReplayEngine` |
| `engine::settlement_engine` | `allocate_discount_by_ratio` / `by_quantity` / `equal` | `pub fn ...`（`settlement_engine.rs:306-391`） | 折扣额/基准 | `Vec<DiscountShare>` | — | 结算引擎内部/测试 |
| `replay::replay_engine` | `ReplayEngine::new` / `replay` | `fn new()->Self`；`async fn replay(&self, round_config:RoundConfig, events:Vec<EventEnvelope>, options:ReplayOptions) -> Result<ReplayResult, ReplayError>` | `RoundConfig`、事件、`ReplayOptions` | `ReplayResult` | `ReplayError::SnapshotRestoreFailed` | 重放步进（**已接线 `/api/replay`**，Wave 1–4；见 §8） |
| `replay::state_diff` | `StateDiff::from_snapshots` | `fn from_snapshots(before:&AllocationSnapshot, after:&AllocationSnapshot) -> StateDiff` | 前后快照 | `StateDiff` | — | `ReplayEngine::replay` |
| `round` | `phase_at` | `fn phase_at(windows:&[PhaseWindow], timestamp_ms:i64) -> Option<RoundPhase>` | 时间窗、时间戳 | 阶段 | — | 已接入实时 Pipeline 与重放（T-13） |
| `round` | `can_claim` / `can_cancel` | `fn(phase:RoundPhase, class:ItemClass, is_priority:bool) -> bool` | 阶段/类别/优先 | bool | — | 已接入（T-13） |
| `messages` | `MessageLog::{append,read_all,replace_all,query,update,delete}` | `MessageLog`（Arc，JSONL 持久化） | `MessageRecord` | `()`/`Vec<MessageRecord>` | `anyhow` | **已实现**（Wave 2/C-5，见 §8.3） |
| `settings` | `ConfigStore::{load,get,put,reload,spawn_watch}` | `load(path)->Result<Self>`；`get()->AppConfig`；`put(cfg,expected)->Result<u64,ConfigError>`；`reload()->Result<u64>`；`spawn_watch()` | `AppConfig` | revision/配置 | `ConfigError::StaleRevision` | `main.rs`、api、pipeline、gateway |
| `settings` | `default_config` / `in_priority_window` / `is_priority_user` | `fn()->AppConfig`；`fn(Option<(i64,i64)>,i64)->bool`；`fn(&[String],&[&str])->bool` | — | 配置/布尔 | — | pipeline、单测 |
| `parser` | `RuleParser::parse` | `fn parse(raw:&str, items:&[Item], _is_admin:bool) -> ParsedMessage` | 文本/商品 | `ParsedMessage` | — | `Pipeline::process`（`pipeline.rs:142`） |
| `parser` | `EventValidator::{new,validate}` | `fn new(f32)->Self`；`async fn validate(parsed,user_id,group_id,raw_message_id,active_rounds,now,sequence) -> AppResult<ValidationOutcome>` | `ParsedMessage`、上下文 | `ValidationOutcome{Ok,NeedConfirm,Reject,Ignore}` | `AppResult` | `Pipeline::process`（`pipeline.rs:207`） |

### 2.2 HTTP 路由（`api`，基址 `127.0.0.1:21081`，装配 `src/api/mod.rs:57-80`）

| 方法 | 路径 | 输入 DTO | 输出 DTO | 错误 | 调用方（web/脚本） | 源码 |
|---|---|---|---|---|---|---|
| GET | `/api/health` | — | `{status,version}` | — | `index.html` | `board_routes.rs:12,17` |
| GET | `/api/board` | — | `{version,board,status}` | — | display/sim | `board_routes.rs:13,21` |
| GET | `/api/gateway/status` | — | `{listening,bound_addr,clients,last_error}` | — | display | `board_routes.rs:14,27` |
| GET | `/api/config` | — | `{config,revision}` | — | admin/display/sim | `config_routes.rs:16,20` |
| PUT | `/api/config` | `{config: AppConfig, revision: u64}` | `{config,revision}` | `409 {error:"stale_revision",revision}`；`500 {error}` | admin | `config_routes.rs:16,32` |
| POST | `/api/config/reload` | `{}` | `{config,revision}` | `500 {error}` | admin | `config_routes.rs:17,45` |
| GET | `/api/display?since=<version>` | query `since` | `{version,board,messages,who_whats,status,changed}` | — | display | `display_routes.rs:38,48` |
| GET | `/api/messages?since=<seq>` | query `since` | `{messages,version}` | — | sim/display | `display_routes.rs:39,77` |
| POST | `/api/sim/message` | `{user_id,nickname,text,offset_ms?,group_id?,is_admin?}` | `{outcome,board,version}` | —（业务态在 `outcome.status`） | `web/sim.js`（sim 页已改 **WS 客户端**；此 HTTP 路由保留给脚本/测试） | `sim_routes.rs:29,47` |
| POST | `/api/sim/identity` | `{user_id,nickname,is_admin?,priority?}` | `{ok,identity,identities}` | — | `web/sim.js:209` | `sim_routes.rs:30,84` |
| POST | `/api/sim/reset` | `{}` | `{ok,version}` | — | `web/sim.js:257` | `sim_routes.rs:31,97` |
| GET | `/api/members` | — | `{members,source:"seed"\|"example"\|"empty"}` | — | admin/sim | `member_routes.rs:51,55` |
| POST | `/api/members/refresh` | `{}` | `{members,source:"gateway"}` | `502 {error}` | admin | `member_routes.rs:52,91` |
| GET | `/api/replay`、`/api/replay/*` | 见 §8 | `ReplayResult` / diff | — | replay 页 | `src/api/replay_routes.rs`（**已实现**，Wave 1–4） |
| GET | `/` `/admin` `/sim` `/replay` | — | 静态 HTML | 404 | 浏览器 | `api/mod.rs:72-75` |
| GET | `/web/*` + fallback | — | 静态文件 | 404 | 浏览器 | `api/mod.rs:76-77` |

### 2.3 前端接口（`web`，仅经 HTTP，不直读引擎）

| 模块 | 名称 | 契约 | 源码 |
|---|---|---|---|
| `web/common.js` | `PAIGU.get(path)` / `PAIGU.post(path,body)` | 统一 fetch 封装（8s 超时、错误分类、CORS `127.0.0.1:*`） | `web/common.js:114-115` |
| `web/common.js` | `normalizeBoard` / `normalizeChanged` | 把 `/api/display` 的 `board`/`changed` 归一化为渲染输入（兼容 local `item_allocations` 与 remote `PublicSnapshot`） | `web/common.js`（见 `web/README.md:70`） |
| `web/common.js` | `createBoardRenderer(host,{persistent})` | keyed diff 渲染器，复用 DOM、3s 高亮（replay 页 `persistent`） | `web/common.js`（`web/README.md:70`） |
| `web/display.js` | 轮询循环 | `GET /api/config` → 每 `refresh_ms` `GET /api/display?since=<version>` | `web/display.js:290,337` |
| `web/admin.js` | 配置读写 | `GET/PUT /api/config`、`POST /api/config/reload`、`GET /api/members`、`POST /api/members/refresh` | `web/admin.js:279-371` |
| `web/sim.js` | 模拟器（**真 WS 客户端**，连同一 Gateway 发 OneBot 事件；Wave 1–4） | `GET /api/members` + WS `ws://…:9801`（HTTP `/api/sim/*` 保留） | `web/sim.js` |
| `web/replay.js` | 重放步进 | **静态文件** `fetch('replay_view.json')`（不读 `/api/display`） | `web/replay.js:1-60`、`web/README.md:69` |

---

## 3. 数据契约

> 标注「必填」指反序列化时无默认值（缺失即报错）；`#[serde(default)]` 视为可选。

### 3.1 `IncomingEvent`（`src/bus.rs:4-13`）
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `group_id` | `String` | 群号 | 是 |
| `user_id` | `String` | QQ user_id（身份真源） | 是 |
| `nickname` | `String` | 已清洗的 display（`clean_nickname` 输出） | 是 |
| `message_id` | `String` | OneBot 消息 ID（幂等键） | 是 |
| `text` | `String` | 规范化纯文本 | 是 |
| `timestamp_ms` | `i64` | 毫秒时间戳（阶段/权限判定基准） | 是 |
| `is_admin` | `bool` | 群主/管理员 | 是 |
| `raw` | `Option<serde_json::Value>` | 原始 OneBot 事件 JSON（C-3 事件溯源/审计） | 否（`Option`） |

### 3.2 `MessageRecord`（`src/messages/mod.rs:4-16`）+ `MessageStore`
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `seq` | `i64` | 全局单调序号 | 是 |
| `group_id` | `String` | 群号 | 是 |
| `user_id` | `String` | user_id | 是 |
| `nickname` | `String` | 显示名 | 是 |
| `message_id` | `String` | 消息 ID | 是 |
| `text` | `String` | 原文 | 是 |
| `timestamp_ms` | `i64` | 时间戳 | 是 |
| `is_admin` | `bool` | 管理员 | 是 |
| `routed` | `String` | 路由判定（如 `Message`/`Drop`） | 是 |
| `status` | `String` | 处理状态（同 `PipelineOutcome.status`） | 是 |
| `detail` | `String` | 详情/摘要 | 是 |

`MessageLog`（**Wave 2 已实现**，取代旧 `MessageStore` trait）：`append` / `read_all` / `replace_all` + 细粒度 `query` / `update` / `delete`，另有 `append_raw_event` / `read_raw_events`（详见 §8.3）。

### 3.3 阶段模型 `RoundPhase / PhaseWindow / ItemClass`（`src/round/mod.rs`）
| 类型 | 取值/字段 | 含义 |
|---|---|---|
| `RoundPhase` | `Phase0, PhaseI, PhaseII, PhaseIII, Settling, Locked` | 拼团六阶段（`:3-11`） |
| `PhaseWindow` | `phase: RoundPhase`、`start_ms: i64`、`end_ms: i64` | 时间窗（`[start,end)`，`:13-18`） |
| `ItemClass` | `A`（盲盒/变体，不可单买）、`B`（固定价单领） | 商品类别（`:20-24`） |

权限矩阵（`can_claim`，`:33-44`，`can_cancel` 同）：Phase0 全禁；PhaseI 仅 B；PhaseII B 全部、A 仅优先；PhaseIII/Settling 全部；Locked 全禁。

### 3.4 `SettlementConfig` 及子类型（`src/settlement/mod.rs`，**Wave 1–4 已实现**）
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `pricing` | `Vec<PricingEntry>` | 调价条目 | 否（default） |
| `discounts` | `Vec<DiscountEntry>` | 折扣条目 | 否 |
| `scope_mode` | `ScopeMode` | A/B 范围（含/不含特典） | 否（默认 `ExcludeGift`，`:53-57`） |
| `gift_tiers` | `Vec<GiftTier>` | 特典档位 | 否 |
| `reduce_average` | `ReduceAverageConfig` | 减均选项 | 否 |

- `PricingEntry`（`:17-23`）：`item_id: String`、`variant_id: Option<String>`、`mode: PricingMode{AdjustBy,SetFinal}`、`value: i64`。
- `DiscountEntry`（`:31-39`）：`rule_id`、`kind: DiscountKind{Threshold,WholeOrder}`、`amount: i64`、`threshold: Option<i64>`、`ratio_ppm: Option<i64>`、`shares: i64`（`-1`=全部，`n≥1`=首 n 包）。
- `GiftTier`（`:59-65`）：`tier_id`、`threshold: i64`、`gift_name: String`、`unit_price: i64`。
- `ReduceAverageConfig`（`:67-70`）：`include_gift_price: bool`。

> 既有确定性结算引擎使用另一套 `DiscountRule`（`src/domain/discount.rs:8-37`：`ThresholdDiscount/FixedActualDiscount/ShoppingFund/GiftByThreshold` + `DiscountScope`/`DiscountAllocationPolicy`）。两套并存，**新 `SettlementConfig` 尚未接入引擎**。

### 3.5 `AllocationSnapshot`（`src/domain/snapshot.rs:7-15`）
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `round_id` | `RoundId` | 团 ID | 是 |
| `version` | `i64` | 版本（新栈=已应用事件数，`pipeline.rs:289,430`） | 是 |
| `generated_at` | `DateTime<Utc>` | 生成时间（RFC3339） | 是 |
| `item_allocations` | `Vec<ItemAllocation>` | 商品分配（`boxes/singles/waiting`，`domain/allocation.rs:103-113`） | 是 |
| `user_summaries` | `Vec<UserAllocationSummary>` | 用户汇总（`display_name` 新栈恒空，`allocation_engine.rs:166`） | 是 |
| `warnings` | `Vec<AllocationWarning>` | 告警（新栈恒空） | 是 |

### 3.6 `SettlementSnapshot`（`src/domain/settlement.rs:7-19`）
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `round_id` | `RoundId` | 团 ID | 是 |
| `version` | `i64` | 版本 | 是 |
| `generated_at` | `DateTime<Utc>` | 生成时间 | 是 |
| `user_bills` | `Vec<UserBill>` | 用户账单（`:22-32`） | 是 |
| `item_totals` | `Vec<ItemTotal>` | 商品汇总（`:59-70`） | 是 |
| `discount_applications` | `Vec<DiscountApplication>` | 折扣应用（`:73-78`） | 是 |
| `gross_total` | `MoneyCents` | 折前总额 | 是 |
| `discount_total` | `MoneyCents` | 折扣总额 | 是 |
| `final_total` | `MoneyCents` | 最终总额 | 是 |
| `warnings` | `Vec<SettlementWarning>` | 告警 | 是 |

### 3.7 `SnapshotBundle`（`src/snapshot_bundle/mod.rs`，**Wave 2 已实现：单一 JSON `SnapshotFile`**）
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `manifest` | `serde_json::Value` | 包元信息 | 是 |
| `config` | `serde_json::Value` | 配置快照 | 是 |
| `messages` | `serde_json::Value` | 消息日志 | 是 |
| `events` | `serde_json::Value` | 事件流 | 是 |
| `snapshot` | `serde_json::Value` | 分配快照 | 是 |
| `settlement` | `serde_json::Value` | 结算结果 | 是 |

### 3.8 `PipelineOutcome`（`src/bus.rs:22-29`）
| 字段 | 类型 | 含义 | 必填 |
|---|---|---|---|
| `status` | `String` | `Applied\|Duplicate\|Ignored\|Rejected\|NeedConfirm\|Error`（`pipeline.rs:97,118,126,161,190,199,229,235,242,252,296`） | 是 |
| `detail` | `String` | 详情/摘要 | 是 |
| `reply` | `Option<String>` | 回复文本（仅回 HTTP，不发送到群） | 是 |
| `version` | `i64` | 处理后的版本 | 是 |
| `snapshot` | `Option<serde_json::Value>` | 最新 `AllocationSnapshot` | 是 |

### 3.9 其他冻结契约
- `ReplayOptions`（`replay/replay_engine.rs:279-294`）：`replay_id`、`include_settlement`、`snapshot_interval`。
- `ReplayResult`（`:296-303`）：`replay_id`、`final_snapshot`、`final_settlement`、`steps`、`input_message_count`。
- `ReplayStep`（`:305-322`）：`step_index`、`event_id`、`state_diff`、`allocation_snapshot`、`decision_trace` 等。
- `StateDiff`（`replay/state_diff.rs:8-15`）：`slot_changes/claim_changes/user_total_changes/item_total_changes/settlement_changes`。
- `SettlementInput`（`engine/settlement_engine.rs:11-15`）：`allocation`、`items`、`discount_rules`。

---

## 4. 需求 → 接口映射（C1..C7）

| 需求 | 落点模块 | 关键接口 / 路由 | 关键文件 | 当前状态 |
|---|---|---|---|---|
| **C1** 实时+记录监听白名单成员 | `gateway`、`messages`、`api`、`web` | `decide_route`、`to_incoming_event`、`MessageStore::append`、`GET /api/messages`、`GET /api/display` | `src/gateway/onebot.rs:218,237`、`src/gateway/ws_server.rs:262-276`、`src/messages/mod.rs:19`、`src/api/display_routes.rs:77`、`web/display.js` | 群/成员白名单 ✅；JSONL 持久化 + Drop 落痕 ✅（Wave 1–2；见 §8.3） |
| **C2** 重放 + 可改附加条件 | `llm::pipeline`、`engine::replay`、`replay::{replay_engine,state_diff}` | `rebuild_allocation_snapshot`、`ReplayEngine::replay`、`StateDiff::from_snapshots`、`replay(messages,overrides)`（待建） | `src/engine/replay.rs:199`、`src/replay/replay_engine.rs:25`、`src/replay/state_diff.rs:18` | 事件重放 ✅；`replay(messages,overrides)` + diff ✅（Wave，`src/replay/session.rs` 已建；见 §8） |
| **C3** 解耦 + 接口映射表 | 全部 | 本文档 | `docs/INTERFACES.md` | ✅（本文件） |
| **C4** 管理改数据 + 快照 | `api`、`snapshot_bundle`、`web` | `/api/messages` 增改删（待建）、`SnapshotBundle` 导出/导入（待建）、`PUT /api/config` | `src/api/display_routes.rs`、`src/snapshot_bundle/mod.rs:4`、`src/api/config_routes.rs:32`、`web/admin.js` | config 编辑 ✅；消息编辑 + 快照包 ✅（Wave 2/C-2；见 §8.4） |
| **C5** 全真模拟 WS client | `web`、`gateway` | `web/sim.js` 连同一反向 WS（OneBot 事件），共用 `Gateway→Pipeline` | `web/sim.js`（Wave 已改 **WS 客户端**）、`src/gateway/ws_server.rs` | ✅ **Wave 已实现** |
| **C6** 脚本倍率模拟 | `scripts` | `scripts/sim-run.mjs --speed` 经 WS 回放 | `scripts/sim-run.mjs`（**已创建**） | ✅ **Wave 已实现** |
| **C7** 录制 + 自动重放 | `scripts`、`replay` | `record.jsonl` + `--replay` 自动重放 | `scripts/sim-record.mjs`、`src/replay/**` | ✅ **Wave 已实现** |
| 阶段权限（REQ §2） | `round`、`llm::pipeline` | `phase_at`、`can_claim`/`can_cancel` | `src/round/mod.rs` | 契约✅；已接入实时+重放（T-13，按 `phase_at` 拒绝越权） |
| 结算/下单表（REQ §3） | `settlement`、`engine::settlement_engine`、`planner` | `SettlementConfig`、`evaluate`、`OrderTable` | `src/settlement/**`、`src/planner/**` | 结算 v2 + planner 下单表 ✅（Wave 1；见 §8.1） |

---

## 5. 解耦约束与依赖方向

```
web ──HTTP──▶ api ──▶ llm::pipeline ──▶ engine::{allocation_engine,replay}
                │                          ├─ replay::{replay_engine,state_diff}
                │                          ├─ settlement (SettlementConfig)
                │                          └─ messages (MessageStore)
                └──▶ gateway（仅用于 status / members/refresh 只读动作）
gateway ──EventSink(trait)──▶ pipeline          # 反向依赖：Gateway 不依赖具体 Pipeline
```

**允许的依赖方向**（证据）：
- `web` 仅经 HTTP（`web/README.md:3,78`；`web/common.js:114-115`）。
- `api::ApiState { cfg, pipeline, gateway }`（`src/api/mod.rs:26-30`）→ 只调 `Pipeline`/`Gateway`/`ConfigStore` 公开方法。
- `Pipeline` 依赖 `bus`、`settings`、`engine::replay`、`parser`（`src/llm/pipeline.rs:11-21`）。
- `Gateway` 只依赖 `bus::EventSink`（`src/gateway/ws_server.rs:14`），不 import `Pipeline`。

**禁止**：
- 反向依赖（`engine`/`messages` 不得依赖 `api`/`web`；`gateway` 不得依赖 `pipeline` 具体类型）。
- 跨层直读内部状态（UI/HTTP 不得直接读 `Pipeline.state` 或引擎内部；必须经公开方法/HTTP DTO）。
- `web/**` 不得改 `src/**`、`simulation-corpus/**`（`web/README.md:78`）。

**已在 C-4 删除（Wave 4；下表中的路径均不再存在）**：

| 路径 | 说明 |
|---|---|
| `src/config.rs` | 旧栈 `Config::from_env()`（Postgres），仅 `main.rs:52-57` 未识别子命令回退 |
| `src/app_state.rs` | 旧栈应用状态装配 |
| `src/services/**` | 旧栈服务层 |
| `src/repo/**` | 旧栈 PostgreSQL 数据访问 |
| `src/publisher/**` | `R2Publisher`/`LocalPublisher` 已实现但新栈未构造、未调用（`main.rs:89-124`） |
| `src/ws/**`、`src/inbound/**` | 旧栈入站/WS（新栈用 `src/gateway/**`） |
| `src/api/routes.rs` + `admin_routes.rs`/`public_routes.rs`/`webhook_routes.rs` | 旧栈路由（新栈装配见 `src/api/mod.rs:64-79`） |
| `migrations/**` | 旧栈数据库迁移（新栈不需要 PostgreSQL） |

> 新栈装配入口：`src/main.rs:48-49` → `run_gateway_stack()`（`:89-124`）。

---

## 6. 快照期「待办」清单（§1–§6 快照；多数已在 Wave 1–4 实现）

> 本表为 2026-09-14 快照。下列能力**多数已实现**，现以实现与 [§8](#8-wave-1-4-新增变更接口2026-09-17) 为准。

| 接口/能力 | 目标模块 | 现状（Wave 1–4 后） |
|---|---|---|
| `MessageLog` JSONL 实现（`data/messages/<round>.jsonl`，追加/读取/查询） | `messages` | ✅ 已实现（Wave 2） |
| `MessageLog::query/update/delete`（管理编辑消息） | `messages` | ✅ 已实现（C-5） |
| 成员白名单（群×成员）与 Drop 落日志 | `gateway`、`settings` | ✅ 已实现 + Drop 落日志 |
| `round::{phase_at,can_claim,can_cancel}` 接入 Pipeline + `settings.phases` | `round`、`llm::pipeline`、`settings` | ✅ 已接入实时+重放（T-13） |
| `replay(messages, overrides)` + 结果 diff | `replay`（`src/replay/session.rs`） | ✅ 已实现 |
| `SnapshotBundle` 导出/导入 | `snapshot_bundle` | ✅ 单一 JSON（C-2）；zip 未做 |
| `/api/messages` 增改删 + `/api/replay/*` | `api` | ✅ 已实现 |
| `SettlementConfig` 引擎（调价/折扣/范围/特典/减均） | `settlement` | ✅ 结算 v2（Wave 1） |
| `OrderTable` + DFS/回溯（两策略） | `planner` | ✅ `src/planner/**` 已实现 |
| 结算/下单表 UI | `web/settlement*` + `api` | ✅ 已实现 |
| `web/sim` 改 WS 客户端 | `web/sim*` | ✅ 已改真 WS 客户端 |
| `scripts/sim-run.mjs`（`--speed`） | `scripts` | ✅ 已实现 |
| 录制 `record.jsonl` + `--replay` | `scripts` | ✅ `scripts/sim-record.mjs` |
| e2e（C5/C6/C7 用例） | `tests` | ✅ 11/11（`node tests/e2e/sim.mjs`） |
| `MessageRecord` 与 Pipeline 消息模型统一 | `messages`、`llm::pipeline` | ✅ 共享 `MessageLog`（T-05） |

> 复核：本文件 §1–§6 为 Wave 1–4 前快照；现状以 §8 与源码为准。

---

## 8. Wave 1-4 新增/变更接口（2026-09-17）

> 均已实现并被单测覆盖（cargo test 138 passed）。旧栈（repo/services/ws/publisher/config/app_state/inbound）已删除。

### 8.1 结算（src/settlement/**）

| 接口 | 说明 |
|---|---|
| `evaluate(&SettlementConfig, &OrderTable) -> SettlementResult` | 新减均：C=无折扣商品总价、B=Σ各包实付、G=特典价合计、D=C-B+G，按「件×标价」加权分摊；校验 Σfinal = B-G |
| `SettlementResult.lines: Vec<LineSettlement>` | 逐行 unit_price_cents/total_cents/reduce_cents/final_total_cents/final_unit_cents |
| `SettlementResult.{list_total_cents, paid_total_cents}` | C / B 透明化 |
| `GiftTier{threshold, unit_price, claimed}` | claimed=排谷认购数；granted=min(claimed,P)，P=下单包数 |
| `check_completeness(&OrderTable, &AllocationSnapshot) -> CompletenessReport` | 排包完成校验（每 (item,variant,is_gift) 数量一致） |
| `expected_quantities` / `table_quantities` | 两侧数量统计 |
| `POST /api/settlement/completeness` | 返回 CompletenessReport |
| `POST /api/settlement/evaluate` | 带 allocation 时未完成排包 -> 400 拒绝计算 |

### 8.2 商品目录（src/settings.rs）

> ✅ **2026-09-24 更新**：字段已改为现行口径（`kind`/`class`/变体 A/B），完整清单见 [§8.7](#87-2026-09-24-新增变更接口u1u9)。

| 字段/接口 | 说明 |
|---|---|
| `ItemConfig.{unit_price_cents, kind, class, aliases, max_quantity, variants}` | 原价（分）、**种类**（`拼团/单领/整盒/特典`）、类别（**自动推导**、可显式覆盖）、别名、单领上限、变体。⚠️ `box_size` 已从口径移除（Rust 端暂留兼容字段，见 W-G2-04） |
| `VariantConfig.{unit_price_cents, adjust_cents, capacity, aliases}` | 变体原价 A、调价 B（最终价 **C=A+B**）、容量、别名。⚠️ `pieces` 已从口径移除；`adjust_cents` **仅前端契约**（Rust 端未声明，见 W-G2-01） |
| `RoundSettings::to_unit_prices()` | 商品目录 -> 标价表（结算/planner 取价来源） |
| `GatewayConfig.admin_commands_enabled` | 管理员命令落地执行开关（D-1） |

### 8.3 消息日志（src/messages/**）

| 接口 | 说明 |
|---|---|
| `MessageLog`（Arc，main.rs 注入） | 共享日志：path_for / store_for / append / read_all / replace_all |
| `MessageLog::{query, update, delete}` | 细粒度本地读写（C-5） |
| `MessageLog::{append_raw_event, read_raw_events}` | 原始事件日志 data/events/<round>.jsonl（C-3） |
| `EventSink::messages() -> Arc<MessageLog>` | Gateway 通过 sink 复用同一实例（T-05） |
| `GET /api/events` | 原始事件列表 |

### 8.4 快照（src/snapshot_bundle/**）

| 接口 | 说明 |
|---|---|
| `SnapshotFile{format, computed_at, computed_by, version, ...}` | 单一 JSON 快照（C-2）：原始消息 + 计算结果缓存 + 计算版本/时间 |
| `SnapshotBundle::{export_file, import_file}` | 单文件导出/导入（校验哈希与格式） |
| `POST /api/snapshot/export`（format="file"） | 单文件；缺省目录式 |
| `POST /api/snapshot/import` | path 为文件时自动走 import_file |

### 8.5 配置与命令（src/api/config_routes.rs、src/llm/pipeline.rs）

| 接口 | 说明 |
|---|---|
| `POST /api/config/reply` | 热切换 gateway.reply_enabled（B-1，默认关闭） |
| `POST /api/config/admin-commands` | 热切换 gateway.admin_commands_enabled（D-1） |
| `Pipeline::run_admin_command` | /开团 /锁位 /结团 /状态 /导出；锁定后拒绝排/撤/改 |
| `ParsedIntent::Modify` | 改单：撤销本人该商品既有认购 + 重新认购（cancel_for_modify） |

### 8.6 工作流（`src/services/workflow.rs`、`src/api/board_routes.rs`）

| 接口 | 说明 |
|---|---|
| `GET /api/workflow` | **只读**快照：`round_id/title/group_id/phase/phase_label/phases/phase_config...`（`phases` 见 §8.7）/`phases_configured/priority_window/items/settlement_configured/members_cached/reply_enabled/admin_commands_enabled/revision/gateway/locked/version/events/messages/claims/eligibilities/updated_at` |
| `RoundPhase::{as_str,label}` | 稳定标识与中文标签（`src/round/mod.rs`），供 Stepper 使用；**Stepper 高亮 = 当前模块**（非当前阶段），阶段进度用弱标记 |

---

### 8.7 2026-09-24 新增/变更接口（U1–U9）

> 均为本轮落地；源码 `src/api/rounds_routes.rs`、`src/services/rounds.rs`、`src/settings/rounds.rs`、`src/services/workflow.rs`、`src/services/members.rs`、`src/services/display.rs`、`src/domain/event.rs`。**标注「前端契约 / 后端未实现」者为已知差距，见 [TODOS.md](./TODOS.md)**。

**轮次库（U3）**

| 接口/字段 | 说明 |
|---|---|
| `GET /api/rounds` | `{rounds:[{round_id,title,group_id,items,variants,active,updated_at}], active_round_id}`；按 mtime 降序 |
| `POST /api/rounds` | body `{round_id, title?, copy_from?}`；`round_id` 非空、≤64、仅字母/数字/中文/`_`/`-`；`copy_from` 存在则复制其商品/阶段 |
| `POST /api/rounds/:id/activate` | body `{mode?:"continue"\|"fresh"\|"replay"}`（**默认 continue**）；`continue` 仅切换激活（无既有状态时等价 `fresh`），`fresh` 重置流水线，`replay` 重置并用该轮消息重放；**切换与重放解耦**。返回 `{ok,active_round_id,mode,reset,replayed,version}` |
| `POST /api/rounds/:id/check` | `{ok,issues:[{level,code,message,where}]}`；含 `duplicate_item_id`/`alias_conflict`/`name_conflict`/`category_variant_mismatch`/`class_derived_mismatch`（warn）等 |
| `DELETE /api/rounds/:id` | `{ok,removed}`；激活中的轮次拒绝 |
| `data/rounds/<round_id>.json` | 轮次文件（内容 = `RoundSettings`）；`PAIGU_ROUNDS_DIR` 可覆盖目录 |
| `AppConfig.active_round_id` | `config/app.json` 当前激活轮次 id；启动/热载据此覆盖 `round`（`settings::rounds::resolve_active`） |
| `GET /api/workflow.phases` | 数组 `[{phase,label,start_ms,end_ms}]`；仅供前端 Stepper **弱阶段进度**使用 |

**商品目录 / 成员 CN**

| 接口/字段 | 说明 |
|---|---|
| `ItemCategory`（`settings`） | `Group/Single/Box/Gift`；`parse` 兼容中英与旧值；`requires_variants()` = 拼团/特典为真 |
| `ItemConfig::{category,has_variants,derived_class}` | 种类解析、是否有变体、`class` 自动推导（有变体⇒A、无变体⇒B） |
| `RoundSettings::item_class` | 显式合法 `class` 可覆盖推导；未配置商品按 `B` |
| `MembersSettings.cn_overrides: Vec<CnOverride>` | `CnOverride{user_id, cn, aliases[]}`（U4；注意字段名是 `MembersSettings`） |
| `settings::resolve_cn` | 展示人名回退 **CN → 归一化昵称（identity） → user_id** |
| `GET /api/members` | 每项装饰 `cn`（覆盖值或 `null`）与 `resolved`（`resolve_cn` 结果） |
| `ParseOverrideEvent`（`domain/event.rs`） | `{event_id,round_id,target_raw_message_id,corrected_parsed_message,admin_user_id,reason,occurred_at}`；事件类型 `parse_override`，重放消费（U7） |
| `POST /api/items/suggest-aliases` | body `{items:[{item_id,name,aliases}], mode:"aliases"}` → `{suggestions:[{item_id,verdict?,aliases?}]}`。⚠️ 后端已实现（`src/api/item_routes.rs`）：`web/round.js` 已接入并对 404/405/501 兜底为「未就绪」；落库接线见 [TODOS.md](./TODOS.md) W-G2-02 |
| `VariantConfig.adjust_cents` | 变体调价 B（最终价 C = A+B）。⚠️ **前端契约**（`web/round.js` 已按 A±B=C 三格联动）；**Rust `VariantConfig` 尚未声明该字段**（当前忽略、不落库）——见 [TODOS.md](./TODOS.md) W-G2-01 |

**页面（静态文件）**

| 路由 | 说明 |
|---|---|
| `/round.html` | 轮次与商品管理页（`web/round.html` + `round.js`）：轮次管理、商品目录编辑器（4 种类、A±B=C、别名本地词切+LLM 建议+锁定+冲突校验、末尾虚线卡/缝隙「+」插入、单领多行、整盒独立种类）、消息日志折叠面板 |
| `/settings.html` | 其余配置页（`web/settings.html` + `settings.js`）：gateway/llm/display/members(CN)/白名单等；**不进 Stepper** |
| — | ⚠️ **无 `/round`、`/settings` 短路由**；两页经静态文件服务（`/round.html`、`/settings.html`）访问。若要短路由需在 `src/api/mod.rs` 增补 |
