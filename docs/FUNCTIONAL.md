# 排谷机器人 · 功能描述文档（FUNCTIONAL）

> 面向评审。本文描述**当前代码实际实现**的功能，用于据此提出修改指示。
> 权威政策见 [POLICY.md](./POLICY.md)，程序路线见 [DESIGN.md](./DESIGN.md)，任务拆分见 [TASKS.md](./TASKS.md)，协作规则见 [AGENT-RULES.md](./AGENT-RULES.md)。冲突时以 POLICY 为准。
>
> **事实来源**：本文所有描述均来自源码阅读（引用 `文件:行号`）与本机运行 `target/debug/paigu-bot-core.exe run`（端口 32181，`llm.enabled=false`）的实测响应（2026-09-14）。JSON 示例为实测输出或按序列化契约裁剪；不确定处标注「待确认」。**本文不修改任何代码或其它文件。**

---

## 1. 概述与定位

- **本地数据处理 + 远程展示**：本地机（`192.168.100.2`）接收 QQ 群消息、LLM 解析、排谷计算；排位快照/回放将来发布到 Cloudflare（R2 + Pages）供成员查看（`docs/POLICY.md:8`）。
- **绝不主动发消息给真实群**：`reply_enabled=false`（`config.example.json:7`）默认关闭；`send_*` 仅当 `reply_enabled=true` 且 `action ∈ allowed_actions` 时放行（已强制，见 §10.1）。成员名单拉取为只读动作，允许。
- **确定性优先**：LLM 只做自然语言 → 结构化；排序/分配/结算由确定性引擎完成（`docs/POLICY.md:10`）。
- **与文档的关系**：POLICY 是权威业务规则；DESIGN 定义模块/接口/路由/部署；TASKS 记录分工与完成状态。本文是**功能视角**的描述（触发 → 处理 → 结果 + JSON），与 DESIGN §5 的接口契约一一对应。
- **新栈装配**：`src/main.rs` 的 `run` 子命令装配 Gateway + Pipeline + HTTP API + 每日成员调度（`src/main.rs:82-117`）。接口冻结件为 `src/bus.rs`、`src/settings.rs`、`config.example.json`（`docs/TASKS.md:83`）。

---

## 2. 运行时架构

### 2.1 进程 / 端口 / 线程

- **单进程**：`#[tokio::main]` 异步运行时（`src/main.rs:31`）。`run` 模式下不连接 PostgreSQL。
- **反向 WebSocket 服务器（Gateway）**：绑定 `AppConfig.gateway.bind`，默认 `192.168.100.2:9801`（`config.example.json:4`；绑定逻辑 `src/gateway/ws_server.rs:46-73`）。NapCat 主动连入。
- **HTTP API**：绑定 `127.0.0.1:{PAIGU_HTTP_PORT}`，默认 `21081`（`src/api/mod.rs:80-87`；`src/main.rs:100-103`）。
- **成员调度任务**：一个 `tokio::spawn` 循环，每日在 `members.daily_pull_at`（默认 `19:00`）拉取群成员（`src/main.rs:143-158`）。
- **每条消息一个任务**：Gateway 对路由通过的每条消息 `tokio::spawn(sink.handle(...))`（`src/gateway/ws_server.rs:267-269`）。Pipeline 共享状态由 `tokio::sync::Mutex` 保护（`src/llm/pipeline.rs:34`），因此业务状态应用是串行的，但多消息的**加锁顺序不等于到达顺序**（并发下 `sequence` 由加锁时刻决定，`src/llm/pipeline.rs:82-112`）。
- **连接内**：每连接一个读循环 + 一个写任务 + 心跳 `interval`（`heartbeat_secs`，默认 15s）（`src/gateway/ws_server.rs:171-184`）。

### 2.2 数据流图

```
NapCatQQ ──反向 WS──▶ Gateway 192.168.100.2:9801
                       │  decide_route（白名单/群消息/非空）→ Drop 记 debug
                       ▼
              IncomingEvent（每消息 spawn）
                       ▼
   Pipeline.process：幂等 → 规则快路径 → LLM 清理/抽取 → EventValidator → 权限(时段/预存)
                       ▼
       内存事件列表 → rebuild(ReplayService + AllocationEngine) → AllocationSnapshot
                       ▼
   axum HTTP API 127.0.0.1:21081
     /api/config /api/board /api/display /api/messages /api/sim/* /api/members /api/gateway/status
     / /admin /sim /web/* 静态页
                       ▼
   浏览器展示页（local 数据源，5s 增量轮询）
   ┈┈┈┈（未接线）┈┈┈┈▶ Cloudflare R2(快照/回放) + Pages(静态展示)
```

- 入队/背压：DESIGN §7 描述有界 `mpsc`；当前 Gateway 实际用 `tokio::spawn` 直接调用 sink（`src/gateway/ws_server.rs:267-269`），**未实现有界队列与背压日志**（待确认是否按 DESIGN 补齐）。
- 发布：`R2Publisher`/`LocalPublisher` 已实现但 `run` 栈未构造、未调用（见 §9、§10）。

### 2.3 端到端时序（一条群消息 → 排位更新）

1. NapCat 建立 WS 连接（`src/gateway/ws_server.rs:168`）。
2. 收到文本帧 → `on_text`：先尝试按 `echo` 回包（动作响应），否则解析 `RouteMessageEvent`（`src/gateway/ws_server.rs:230-249`）。
3. `decide_route`：`post_type=="message"` 且 `message_type=="group"` 且 `group_id∈whitelist` 且规范化文本非空，否则 `Drop`（`src/gateway/onebot.rs:218-235`）。
4. `to_incoming_event`：解析身份、`timestamp_ms = time*1000`（无 time 用当前时间），生成 `IncomingEvent`（`src/gateway/onebot.rs:237-256`）。
5. `Pipeline.process`（`src/llm/pipeline.rs:73`）：
   1. 幂等键 `group_id::message_id`，重复 → `Duplicate`（`:84-107`）；
   2. 空文本 → `Ignored`（`:116-120`）；
   3. 以 `/` 开头 → 管理员命令分支（`:122-145`）；
   4. `RuleParser::parse`，规则高置信（`>=0.9` 且非 Unknown）直接用，否则调 LLM；LLM 失败按 `fallback_to_rules` 回退（`:147-186`）；
   5. `EventValidator::validate`：Unknown→Ignore、歧义→NeedConfirm、置信度<0.65→Reject、数量 0/>99→Reject（`:213-255`；`src/parser/validation.rs:38-104`）；
   6. 权限：优先时段内非预存 → `Rejected`（`:257-269`）；
   7. 追加事件 → `rebuild` 重放得到快照，`version = 事件数`（`:271-302`；`src/llm/pipeline.rs:416-432`）。
6. 返回 `PipelineOutcome { status, detail, reply, version, snapshot }`（`src/bus.rs:23-30`）。`reply` 仅回给 HTTP 调用方，不发送到群。
7. 展示页每 5s `GET /api/display?since=<version>` 拉增量并 keyed diff（`web/display.js:290`；`src/api/display_routes.rs:48-75`）。

---

## 3. 运行模式与入口

入口分派见 `src/main.rs:37-48`。

| 模式 | 命令 | 效果 |
|---|---|---|
| `run`（默认，新栈） | `cargo run` 或 `cargo run -- run` | Gateway(`gateway.bind`, 默认 `192.168.100.2:9801`) + Pipeline + API(`127.0.0.1:21081`) + 每日 19:00 成员调度（`src/main.rs:46-48,82-117`） |
| `simulate`（离线重放验证，旧引擎） | `cargo run -- simulate --round-config <json> --queue <jsonl> [--out <dir>]` | 逐条解析/校验/重放语料，输出 `out/report.md`、`out/result.json`、`out/outcomes.jsonl`（`src/main.rs:38-41`；`src/simulation/verifier.rs:62-93`） |
| `serve`（本地聊天服务器，旧引擎） | `cargo run -- serve --round-config <json> [--port 8090]` | 内存会话聊天页 + 实时排结果，无 PostgreSQL（`src/main.rs:42-45`；`src/simulation/chat_server.rs:302-323`） |
| `legacy`（旧栈，弃用） | `cargo run -- <任意未识别子命令>` | **无 `legacy` 字面分支**；任何非 `simulate/serve/run` 的子命令落入旧栈：读取 `DATABASE_URL` 等环境变量、连接 PostgreSQL、WS 服务 `3001`、HTTP API `8080`（`src/main.rs:50-79`；`src/config.rs:52-108`）。需数据库，已弃用 |

> 说明：`cargo run -- legacy` 并不被专门识别，而是走「未识别子命令 → 旧栈」路径（`src/main.rs:50` 起）。旧栈使用 `src/api/routes.rs` 的路由（`/webhook`、`/admin`、`/public`、`/api/replays`、`/api/simulations`），与新栈 `src/api/mod.rs` 不同。

---

## 4. 配置

### 4.1 `AppConfig` 全字段表

定义见 `src/settings.rs:12-186`；默认值来自 `config.example.json`（经 `include_str!` 嵌入，`src/settings.rs:183-186`）。「热载」指 `PUT /api/config` 或文件变更后是否即时生效。

| 字段路径 | 类型 | 默认值 | 含义 | 热载 |
|---|---|---|---|---|
| `revision` | u64 | `1` | 乐观并发版本号 | 是（`put`/`reload` 更新） |
| `gateway.bind` | String | 无（必填） | 反向 WS 监听地址 | 部分：`run` 循环只在 accept 返回后重读，监听中不重绑（`src/gateway/ws_server.rs:46-73`） |
| `gateway.whitelist_groups` | Vec<String> | `[]` | 白名单群号 | 是（每帧读配置，`src/gateway/ws_server.rs:251-254`） |
| `gateway.heartbeat_secs` | u64 | `15` | 心跳间隔 | 否（每连接读一次，`src/gateway/ws_server.rs:182`） |
| `gateway.reply_enabled` | bool | `false` | 是否允许回复群消息 | 是（每次 `send_action` 读；`send_*` 仅当 `true` 且 `action∈allowed_actions` 放行，`src/gateway/ws_server.rs:75-88`；§10.1） |
| `gateway.allowed_actions` | Vec<String> | `[]` | 允许的出站动作白名单 | 是（每次 `send_action` 读，`src/gateway/ws_server.rs:79-82`） |
| `llm.enabled` | bool | `true` | 是否启用 LLM | 是（每消息读，`src/llm/pipeline.rs:152`） |
| `llm.base_url` | String | 无（必填） | OpenAI 兼容 base URL | 是 |
| `llm.model` | String | 无（必填） | 模型名 | 是 |
| `llm.api_key_env` | String | 无（必填） | 读取 API key 的环境变量名 | 是 |
| `llm.timeout_secs` | u64 | `60` | 请求超时 | 是 |
| `llm.max_tokens` | u32 | `2048` | 最大输出 token | 是 |
| `llm.temperature` | f32 | `0.1` | 采样温度 | 是 |
| `llm.fallback_to_rules` | bool | `true` | LLM 失败时回退规则解析 | 是 |
| `llm.prompt_template` | String | `""`（空则用 `DEFAULT_PROMPT`） | 系统提示词 | 是（`src/llm/prompt.rs:6-11`） |
| `round.round_id` | String | 无（必填） | 团 ID | 是 |
| `round.title` | String | 无（必填） | 团标题 | 是 |
| `round.group_id` | String | 无（必填） | 群号 | 是 |
| `round.priority_users` | Vec<String> | `[]` | 预存（购物金）用户 | 是（每消息读，`src/llm/pipeline.rs:494-499`） |
| `round.priority_window` | Option<{start_ms,end_ms}> | `None` | 优先时段（end 独占） | 是（`src/llm/pipeline.rs:487-492`） |
| `round.items` | Vec<ItemConfig> | `[]` | 商品目录 | 是（`src/settings.rs:113-152`） |
| `round.items[].item_id` | String | 必填 | 商品 ID | 是 |
| `round.items[].name` | String | 必填 | 商品名 | 是 |
| `round.items[].kind` | String | 必填 | `split`/`single`/`gift`/`shipping`/`adjustment`（其它按 split） | 是（`src/settings.rs:123-129`） |
| `round.items[].aliases` | Vec<String> | `[]` | 商品别名 | 是 |
| `round.items[].variants` | Vec<VariantConfig> | `[]` | 变体 | 是 |
| `…variants[].variant_id` | String | 必填 | 变体 ID | 是 |
| `…variants[].name` | String | 必填 | 变体名 | 是 |
| `…variants[].capacity` | Option<u32> | `None` | 盒容量（`null` 时 FullBox/包尾取消息数量） | 是 |
| `…variants[].aliases` | Vec<String> | `[]` | 变体别名 | 是 |
| `display.refresh_ms` | u64 | `5000` | 展示页轮询间隔 | 是（前端载入时读，`web/display.js:337`） |
| `display.data_source` | String | `"local"` | `local`/`remote` | 是 |
| `display.remote_base_url` | String | `""` | 远程快照基址 | 是 |
| `members.group_id` | String | 必填 | 拉取成员的群号 | 是 |
| `members.cache_path` | String | 必填 | 成员缓存文件 | 是 |
| `members.daily_pull_at` | String | `"19:00"` | 每日拉取时刻 | 是（调度每轮读，`src/main.rs:146`） |

> 注意：`round.items[].box_size`、`max_quantity`、`unit_price` 等 `Item` 字段**不在配置中**，由 `to_items()` 置零/`None`（`src/settings.rs:130-137`）。因此 FullBox/包尾在 `capacity=null` 时使用消息数量作为盒规（`src/engine/allocation_engine.rs:203,241-244`），单领上限恒为无限（`max_quantity=None`，`src/engine/allocation_engine.rs:417-436`）。

### 4.2 环境变量

| 变量 | 作用 | 引用 |
|---|---|---|
| `PAIGU_CONFIG_PATH` | 配置文件路径，默认 `config/app.json` | `src/main.rs:83` |
| `PAIGU_HTTP_PORT` | HTTP API 端口，默认 `21081` | `src/main.rs:100-103` |
| `PAIGU_WEB_DIR` | 静态页目录，默认 `web` | `src/api/mod.rs:32-36` |
| `DEEPSEEK_API_KEY` | LLM key（由 `llm.api_key_env` 指定名） | `src/llm/client.rs:84-85`；`config.example.json:20` |
| `RUST_LOG` | tracing 过滤 | `src/main.rs:33-35` |
| 旧栈专用 | `DATABASE_URL`、`DATABASE_MAX_CONNECTIONS`、`R2_*`、`LLM_*`、`HOST/PORT/WS_*` | `src/config.rs:52-108` |

### 4.3 `config/app.json` 生成与热载机制

- **生成**：若 `PAIGU_CONFIG_PATH` 指向的文件不存在，用内嵌的 `config.example.json` 写出（`src/settings.rs:216-227`）。当前仓库已存在 `config/app.json`（`revision:1`），内容与示例一致（`config/app.json:1-40`）。
- **存储**：`ConfigStore` = `RwLock<AppConfig>` + `path`（`src/settings.rs:208-211`）。
- **写**：`put(cfg, expected)` 校验 `expected==当前 revision`，不符返回 `StaleRevision{expected,actual}`，相符则 `revision+1` 落盘并替换内存（`src/settings.rs:246-260`）。
- **重载**：`reload()` 从磁盘读取；若文件 revision `<=` 内存则强制 `+1`（避免回退）（`src/settings.rs:262-272`）。
- **监听**：`spawn_watch()` 用 `notify` 监听文件，300ms 去抖后异步 `reload()`（`src/settings.rs:275-308`）。DESIGN §9 提到的「无依赖时退化为 mtime 轮询」**未实现**（`notify` 已在依赖中，`Cargo.toml`）。
- **revision/notify**：`PUT /api/config` 成功后前端刷新 revision（`web/admin.js:298-303`）；文件热载不会主动推送前端，前端需重新 `GET /api/config`（`web/admin.js:279`）。

---

## 5. 功能清单

> 每条格式：**触发 → 处理 → 结果**，附实测/契约 JSON。`PipelineOutcome.status` 取值集合为 `Applied | Duplicate | Ignored | Rejected | NeedConfirm | Error`（`src/llm/pipeline.rs:97,118,126,161,190,199,229,235,242,252,296`）。

### F1 接入与白名单

- **触发**：NapCat 反向连接 `ws://192.168.100.2:9801` 并上报群消息文本帧。
- **处理**：`accept_async` → `on_text`（先尝试 echo 回包）→ `RouteMessageEvent` 反序列化 → `decide_route`（`post_type=="message"`、`message_type=="group"`、`group_id∈whitelist`、`normalize_message` 非空）；非路由项 `Drop` 仅记 debug（`src/gateway/ws_server.rs:230-263`；`src/gateway/onebot.rs:218-235`）。消息文本由 `message` 段的 text 拼接，或从 `raw_message` 去 CQ 码并反转义（`src/gateway/onebot.rs:97-150`）。
- **结果**：路由通过 → `IncomingEvent` → spawn 交给 Pipeline。出站动作仅允许 `allowed_actions`；`send_*` 仅当 `reply_enabled=true` 且在白名单时放行（默认 `false` → 拒绝并告警，`src/gateway/ws_server.rs:75-88`）。
- **入站事件 JSON 示例**（OneBot 字段）：
```json
{
  "post_type": "message",
  "message_type": "group",
  "self_id": 3000000000,
  "user_id": 10001,
  "group_id": 720675572,
  "message_id": 42,
  "raw_message": "排 通行证 结城理 1",
  "message": [{ "type": "text", "data": { "text": "排 通行证 结城理 1" } }],
  "sender": { "user_id": 10001, "nickname": "成员01", "card": "成员01（备注）", "role": "member" },
  "time": 1788782400
}
```
- **出站动作帧（只读）**：`{ "action": "get_group_member_list", "params": { "group_id": "720675572" }, "echo": "<uuid>" }`（`src/gateway/ws_server.rs:94`；`src/gateway/action.rs:7-10`）。

### F2 昵称清洗与身份

- **触发**：事件携带 `sender.card` 或 `sender.nickname`。
- **处理**：`parse_identity` 取 `user_id`（顶层或 sender），昵称优先用 `card`；`clean_nickname` 做全角→半角归一、识别代理写法 `A(代B)` → 身份 `A` / 显示 `A(B)`、去首个括号备注（`src/gateway/onebot.rs:187-216`）；`is_admin = role ∈ {owner, admin}`（`:209`）。Pipeline 在入口直接调用同一个 `gateway::onebot::clean_nickname`（**昵称清洗单一真源**，`src/llm/pipeline.rs:73`）。
- **结果**：`Identity { user_id, identity, display, is_admin }`；`to_incoming_event` 把 `display` 写入 `nickname`（`src/gateway/onebot.rs:246-255`）。
- **示例**：
  - `成员01（备注）` → 身份/显示 `成员01`
  - `A（代B）` → 身份 `A`，显示 `A(B)`
  - `名：015` → `名:015`
  - 测试：`src/gateway/onebot.rs:351-369`

### F3 消息判定（规则快路径 + LLM 清理/抽取 + 回退）

- **触发**：Pipeline 收到 `IncomingEvent`。
- **处理**：
  1. 幂等：键 `group_id::message_id`，命中 → `Duplicate`（`src/llm/pipeline.rs:84-107`）。
  2. 空文本 → `Ignored`（`:116-120`）。
  3. `/` 开头：非管理员 → `Rejected`「非管理员斜杠命令」；管理员 → `Applied`「管理员命令已记录」（`:122-145`，仅记录不执行）。
  4. `RuleParser::parse`（规则置信度 `>=0.9` 且 intent 非 Unknown 则采用）；否则 `llm.enabled` 时调 LLM；LLM 抛错且 `fallback_to_rules` → 回退规则，否则 `Rejected`「没识别成功」（`:147-186`；常量 `:28-29`）。
  5. `EventValidator`：Unknown→`Ignore`；有 `ambiguous_parts`→`NeedConfirm`；`confidence<0.65`→`Reject`；`quantity==0` 或 `>99`→`Reject`；商品无法解析→`Reject`（`src/parser/validation.rs:38-104`）。
  6. `Modify` 意图 → `Ignored`「改单功能暂未实现」（`src/llm/pipeline.rs:188-192`）。
- **LLM 输出契约**（`src/llm/prompt.rs:43-50`；解析 `src/llm/pipeline.rs:539-615`）：
```json
{
  "intent": "claim",
  "items": [
    { "item": "通行认证SP-月行水上", "variant": "岳羽由加莉", "quantity": 1,
      "claim_type": "split", "slot_policy": "normal" }
  ],
  "confidence": 0.95,
  "ambiguous_parts": []
}
```
> 说明：契约文档用 `variant:"岳羽由加莉"`；解析器实际同时接受 `item`/`name`/`variant`，按商品名/别名/ID 匹配（`src/llm/pipeline.rs:617-690`）。`slot_policy` 实际值为 `normal|tail|fullbox`（提示词），归一化后映射为 `Normal|TailLocked|FullBox`（`src/parser/normalize.rs:17-31`）。
- **结果示例**（实测 `POST /api/sim/message` 的 `outcome`）：
```json
{
  "status": "Applied",
  "detail": "claim: pass_spx1[normal]",
  "reply": "已记录，当前版本 #1",
  "version": 1,
  "snapshot": { "...": "见 F5/§7" }
}
```
```json
{ "status": "Ignored", "detail": "无法识别为排谷/撤销意图", "reply": null, "version": 0, "snapshot": null }
```

### F4 权限（优先时段 / 预存 / 管理员）

- **触发**：校验通过后。
- **处理**：`is_priority_user` 用 `priority_users` 逐项匹配 `user_id`/`nickname`/`identity`/`display` 任一（`src/llm/pipeline.rs:494-499`）；`in_priority_window` 判定 `timestamp_ms ∈ [start_ms, end_ms)`（end 独占，`:487-492`）。窗口内且非预存 → `Rejected`；预存用户首次成功排谷时注入 `Eligibility{priority_level:10}`（`:257-269,274-278,467-485`）。
- **排序**：`priority DESC → effective_at ASC → sequence ASC`（`src/domain/event.rs:193-199`；`src/engine/replay.rs:124-129`）。
- **结果示例**（实测：非预存用户在窗口内）：
```json
{
  "status": "Rejected",
  "detail": "优先时段仅限预存(购物金)用户",
  "reply": "优先时段仅限预存(购物金)用户，请求已拒绝",
  "version": 0,
  "snapshot": null
}
```
- 管理员斜杠命令：仅 `is_admin` 可用，非管理员 `/` 命令被拒绝（`src/llm/pipeline.rs:122-134`）。

### F5 排谷执行（变体 / 单领 / 包盒 / 包尾 / 撤销 / 幂等）

- **触发**：`ParsedIntent::Claim` 校验为 `Ok(event)` 后。
- **处理**：
  - 变体感知：状态键为 `(item_id, variant_id)`（`src/engine/allocation_engine.rs:40,50-71`）；配置商品 → `Item` 映射见 `src/settings.rs:113-152`。
  - `split` 普通：填第一个可填普通槽，满则开新盒（`src/engine/allocation_engine.rs:222-235`）。
  - `single`：写入 `singles`，超出 `max_quantity` 的部分进 `waiting`（`:417-456`）。
  - `FullBox`（包盒）：独占整盒，未占满的槽标记 `LockedEmpty` 并带 `segment_id`（`:202-220`）。
  - `TailLocked`（包尾）：开新盒占前 N 槽、其余锁空（`:237-258`）。
  - `ColumnLocked`（锁列）：当前按普通槽处理（`:190-192`，见 §10）。
  - 撤销：`ClaimCancelled` 在重放时按「目标 claim → 目标 item 数量 → 最近一条」依次取消（`src/engine/replay.rs:134-165`）。
  - 幂等：同 `message_id` 不重复处理（`src/llm/pipeline.rs:84`）。
- **结果**：重建 `AllocationSnapshot`，`version = 已应用事件数`（`src/llm/pipeline.rs:416-432`）。
- **实测快照 JSON（裁剪：`pass_sp/v_jcl` 被 `成员01` 占第 1 槽）**：
```json
{
  "round_id": "月行水上",
  "version": 1,
  "generated_at": "2026-09-14T04:28:33.172973900Z",
  "item_allocations": [
    {
      "item_id": "pass_sp",
      "item_name": "通行认证SP-月行水上",
      "kind": "split",
      "variant_id": "v_jcl",
      "boxes": [
        {
          "box_index": 1,
          "slots": [
            {
              "slot_index": 1,
              "user_id": "10001",
              "claim_id": "ec5d6724-5a13-4d1a-8feb-0ad212ecdd9f",
              "claim_line_index": 0,
              "status": "Filled",
              "slot_policy": "Normal",
              "segment_id": null,
              "lock_reason": null
            }
          ]
        }
      ],
      "singles": [],
      "waiting": []
    }
  ],
  "user_summaries": [
    {
      "user_id": "10001",
      "display_name": "",
      "items": [
        { "item_id": "pass_sp", "item_name": "通行认证SP-月行水上",
          "quantity": 1, "claim_type": "Split", "unit_price": 0, "gross": 0 }
      ]
    }
  ],
  "warnings": []
}
```
> **序列化注意（重要）**：`SlotStatus`、`SlotPolicy`、`ClaimType` 等枚举以 **PascalCase** 序列化（实测 `"status":"Filled"`、`"slot_policy":"Normal"`、`"claim_type":"Split"`），而 `as_str()` 返回的是 snake_case（`src/domain/allocation.rs:15-24`；`src/domain/claim.rs:13-51`）。前端部分样式判定使用小写（`web/common.js:228-235`），存在不一致（见 §10）。

### F6 快照与重放

- **触发**：每次成功应用事件；或 `GET /api/board`。
- **处理**：事件保存在内存 `State.events`（`src/llm/pipeline.rs:39`）；`rebuild` 先按 `effective_at, sequence` 排序，再 `ReplayService::collect_effective_claims` 得到生效行并按优先级排序，最后 `AllocationEngine::allocate` 生成快照（`src/llm/pipeline.rs:416-432`；`src/engine/replay.rs:63-132`）。`version = 排序后事件数`（`src/llm/pipeline.rs:430`）。
- **结果**：`GET /api/board` 返回 `{version, board, status}`；无事件时 `board:null`。
- **实测（空）**：
```json
{ "board": null, "version": 0,
  "status": { "listening": true, "bound_addr": "127.0.0.1:0", "clients": 0, "last_error": null } }
```
- **限制**：无持久化（重启丢失）；`GET /api/replay` 与 `/api/replay/*` 恒返回 `501 {"error":"not_implemented"}`（`src/api/mod.rs:50-55,69-70`）。

### F7 who-whats

- **触发**：`GET /api/display`（或展示页渲染）。
- **处理**：`who_whats` 遍历快照 `user_summaries`，以 `display` 为分组键（`state.display` 映射，缺失回退 user_id），identity 记录在 `state.identity`，商品按 `item_name` 累加数量，`BTreeMap` 稳定排序（`src/llm/pipeline.rs:338-367`）。前端在无 `who_whats` 时可从排位表推导（`web/common.js:701-724`）。
- **实测 JSON**：
```json
[
  { "display": "成员01", "identity": "成员01",
    "items": [ { "name": "通行认证SP-月行水上", "qty": 1 } ] }
]
```

### F8 成员名单（缓存 / 每日 19:00 / 手动拉取）

- **触发**：`GET /api/members`；每日 `daily_pull_at`；`POST /api/members/refresh`。
- **处理**：
  - `GET /api/members` 读取顺序：`data/members.seed.json`（gitignored，真实名单）→ `data/members.example.json`（入库占位）→ 空数组；`source` 为 `seed|example|empty`（`src/api/member_routes.rs:39-58`）。路径可用 `PAIGU_MEMBERS_SEED_PATH` / `PAIGU_MEMBERS_EXAMPLE_PATH` 覆盖（`:15-25`）。
  - 拉取：`gateway.send_action("get_group_member_list")`（只读）→ 校验 `status=="ok"` 且 `data` 非空 → 写 `members.cache_path`（默认 `data/members.json`）（`src/api/member_routes.rs:60-89`）。
  - 调度：`run` 启动时 spawn 循环，每日在 `daily_pull_at` 调 `refresh_members_from_gateway`，失败仅告警（`src/main.rs:142-157`）。
- **实测 `GET /api/members`（example 兜底，占位名）**：
```json
{ "source": "example",
  "members": [
    { "user_id": null, "nickname": "成员01" },
    { "user_id": null, "nickname": "成员02" },
    { "user_id": null, "nickname": "成员03" }
  ]
}
```
- **拉取失败**：`POST /api/members/refresh` → `502 {"error":"gateway_action_failed: ..."}` 或 `no connected client`（`src/api/member_routes.rs:91-101`；`src/gateway/ws_server.rs:88`）。
- **前端兜底**：API 不可用时 `web/common.js` 使用内置占位子集（`成员01`…`成员05`，`web/common.js:759-769`）。

### F9 展示页（5s 增量 / keyed diff / 不打断 / local-remote）

- **触发**：打开 `/`（`display.html`）或 `/web/display.html`。
- **处理**：`display.js` 初始化 → `GET /api/config` 取 `display.*` → 每 `refresh_ms`（默认 5000）`GET /api/display?since=<version>`（`web/display.js:290,337`）。
  - **keyed diff**：`syncKeyedChildren` 复用 DOM；排位单元格 key = `item|variant#box:slot`；消息 key = `s<seq>`；who-whats key = `display`（`web/common.js:138-166,287-306`）。变化格高亮 3s（`web/common.js:197-210`）。
  - **不打断**：仅在文本变化时写入；`extra` 块用 `__html` 比较后写（`web/common.js:319-322`）；smart-scroll 仅在距底 60px 内自动跟随，否则显示「有新内容 N 条」（`web/common.js:373-424`）。
  - **数据源**：`local` 走 API；`remote` 依次尝试 `<base>/rounds/<round_id>/current`、`<base>/current`（**无 `.json` 后缀**，`web/common.js:841-847`）。
- **实测 `GET /api/display?since=0`（裁剪）**：
```json
{
  "version": 1,
  "board": { "...": "AllocationSnapshot，见 F5" },
  "messages": [
    { "seq": 1, "display": "成员01", "text": "排 通行证 结城理 1",
      "status": "Applied", "detail": "claim: pass_spx1[normal]" }
  ],
  "who_whats": [ { "display": "成员01", "identity": "成员01",
    "items": [ { "name": "通行认证SP-月行水上", "qty": 1 } ] } ],
  "status": { "listening": true, "bound_addr": "127.0.0.1:0", "clients": 0, "last_error": null },
  "changed": []
}
```
- **静态资源实测**：`/sim`、`/common.js`、`/sim.js`、`/display.css` 均返回 HTTP 200（由 `src/api/mod.rs:72-77` 的静态路由与 `fallback_service` 提供）。

### F10 管理面板（revision / 409）

- **触发**：打开 `/admin`。
- **处理**：`load()` → `GET /api/config`（`web/admin.js:279-292`）；保存 → `PUT /api/config {config, revision}`（`:298`）；冲突（HTTP 409）→ 横幅提示「配置冲突，需重新载入」（`:305-307`）；「从磁盘重载」→ `POST /api/config/reload`（`:317`）；成员表 → `GET /api/members`、拉取 → `POST /api/members/refresh`（`:347-371`）。商品编辑器支持 items/variants 增删改（`:118-228`）；时段窗口 `datetime-local` ↔ `start_ms/end_ms`（`:46-62`）。
- **结果**：服务端 `ConfigStore.put` 校验 revision（`src/settings.rs:246-260`），冲突返回：
```json
{ "error": "stale_revision", "revision": 5 }
```
（HTTP 409；`src/api/config_routes.rs:60-71`）

### F11 模拟器（身份 / 时间偏移）

- **触发**：打开 `/sim`。
- **处理**：`GET /api/members` 填充身份下拉（`web/sim.js:296-307`）；切换身份 → `POST /api/sim/identity`（`:201-212`）；输入偏移 `±DD HH MM SS`（`web/common.js:426-446`）；发送 → `POST /api/sim/message`，`timestamp_ms = now + offset_ms`（`src/api/sim_routes.rs:53-64`）；服务端走与真实链路**同一** `Pipeline::process`（`src/api/sim_routes.rs:69`）。重置 → `POST /api/sim/reset`（`src/api/sim_routes.rs:98-103`）。
- **实测请求/响应**：
```json
POST /api/sim/message
{ "user_id": "10001", "nickname": "成员01", "text": "排 通行证 结城理 1", "offset_ms": 0, "is_admin": false }
```
```json
{
  "version": 1,
  "board": { "...": "AllocationSnapshot" },
  "outcome": { "status": "Applied", "detail": "claim: pass_spx1[normal]",
    "reply": "已记录，当前版本 #1", "version": 1, "snapshot": { "...": "AllocationSnapshot" } }
}
```
- **注意**：`POST /api/sim/identity` 仅把身份存入进程内 `BTreeMap`（`src/api/sim_routes.rs:15-25,85-96`），**不参与消息处理**；模拟消息的预存判定实际由 `config.round.priority_users` + 昵称匹配决定（`src/llm/pipeline.rs:494-499`）。`sim.js` 发送的 `priority_level` 字段不在 `SimIdentity` 结构体中，会被忽略（`web/sim.js:203-208`；`src/api/sim_routes.rs:75-83`）。

### F12 配置热载

- **触发**：`PUT /api/config`、`POST /api/config/reload`，或 `config/app.json` 文件变更。
- **处理**：`ConfigStore` 为 `RwLock<AppConfig>`（`src/settings.rs:208-211`）；`spawn_watch` 用 `notify` 监听，300ms 去抖后 `reload()`（`:275-308`）；`reload()` 保证 revision 单调（`:262-272`）。
- **结果**：`GET/PUT /api/config` 返回 `{config, revision}`（`src/api/config_routes.rs:20-43`）。
- **注意**：`gateway.bind` 在监听中不会重绑（`run` 循环只在 `accept_loop` 返回后重读，`src/gateway/ws_server.rs:46-73`）；`heartbeat_secs` 每连接读一次（`:182`）。

---

## 6. HTTP API 参考

基址 `http://127.0.0.1:21081`（`src/api/mod.rs:80-87`）。CORS 允许 `null`、`http://127.0.0.1:*`、`http://localhost:*`（`src/api/mod.rs:38-48`）。路由装配 `src/api/mod.rs:57-78`。

| 方法 | 路径 | 请求体 | 成功响应 | 错误 |
|---|---|---|---|---|
| GET | `/api/health` | — | `{status:"ok", version}` | — |
| GET | `/api/board` | — | `{version, board, status}` | — |
| GET | `/api/gateway/status` | — | `{listening, bound_addr, clients, last_error}` | — |
| GET | `/api/config` | — | `{config, revision}` | — |
| PUT | `/api/config` | `{config, revision}` | `{config, revision}` | `409 {error:"stale_revision", revision}`；`500 {error}` |
| POST | `/api/config/reload` | `{}` | `{config, revision}` | `500 {error}` |
| GET | `/api/display?since=<version>` | — | `{version, board, messages, who_whats, status, changed}` | — |
| GET | `/api/messages?since=<seq>` | — | `{messages, version}` | — |
| POST | `/api/sim/message` | `{user_id, nickname, text, offset_ms?, group_id?, is_admin?}` | `{outcome, board, version}` | — |
| POST | `/api/sim/identity` | `{user_id, nickname, is_admin?, priority?}` | `{ok, identity, identities}` | — |
| POST | `/api/sim/reset` | `{}` | `{ok, version}` | — |
| GET | `/api/members` | — | `{members, source:"seed"\|"example"\|"empty"}` | — |
| POST | `/api/members/refresh` | `{}` | `{members, source:"gateway"}` | `502 {error}` |
| GET | `/api/replay`、`/api/replay/*` | — | — | `501 {error:"not_implemented"}` |
| GET | `/` | — | `display.html` | — |
| GET | `/admin` | — | `admin.html` | — |
| GET | `/sim` | — | `sim.html` | — |
| GET | `/replay` | — | `replay.html` | — |
| GET | `/web/*` | — | 静态文件；未命中回退目录 `web/` | 404 |

**细节与引用**：

- `GET /api/health` → `src/api/board_routes.rs:17-19`；实测 `{"status":"ok","version":"0.1.0"}`。
- `GET /api/board` → `src/api/board_routes.rs:21-25`；空态实测 `{"board":null,"version":0,"status":{...}}`。
- `GET /api/gateway/status` → `src/gateway/ws_server.rs:113-120`。
- `GET /api/config` / `PUT` / `POST reload` → `src/api/config_routes.rs:14-71`；409 映射 `:60-65`。
- `GET /api/display?since=` → `src/api/display_routes.rs:48-75`；`changed` 仅在缓存中命中 `since` 对应版本时计算（`:55-65`）。
- `GET /api/messages?since=` → `src/api/display_routes.rs:77-82`。
- `POST /api/sim/message` → `src/api/sim_routes.rs:34-73`；`group_id` 缺省用 `round.group_id`。
- `POST /api/sim/identity` → `src/api/sim_routes.rs:75-96`。
- `POST /api/sim/reset` → `src/api/sim_routes.rs:98-103`（`pipeline.reset()` + `display_routes::clear_cache()`）。
- `GET /api/members` / `POST /api/members/refresh` → `src/api/member_routes.rs:30-91`。
- 静态页 → `src/api/mod.rs:57-75`。

**curl 示例**：

```bash
# 健康检查
curl http://127.0.0.1:21081/api/health

# 当前排位
curl http://127.0.0.1:21081/api/board

# 增量展示（since 传上次 version）
curl "http://127.0.0.1:21081/api/display?since=0"

# 模拟发送一条排谷消息
curl -X POST http://127.0.0.1:21081/api/sim/message \
  -H "Content-Type: application/json" \
  -d '{"user_id":"10001","nickname":"成员01","text":"排 通行证 结城理 1","offset_ms":0,"is_admin":false}'

# 读取配置
curl http://127.0.0.1:21081/api/config

# 保存配置（revision 必须与服务器一致，否则 409）
curl -X PUT http://127.0.0.1:21081/api/config \
  -H "Content-Type: application/json" \
  -d '{"config":{...},"revision":1}'

# 拉取群成员（只读；需 NapCat 已连接）
curl -X POST http://127.0.0.1:21081/api/members/refresh

# 重置模拟会话
curl -X POST http://127.0.0.1:21081/api/sim/reset -H "Content-Type: application/json" -d '{}'
```

---

## 7. 数据模型与文件

### 7.1 文件

| 路径 | 说明 | 是否入库 |
|---|---|---|
| `config/app.json` | 运行配置，`ConfigStore` 读写；缺失时从内嵌 `config.example.json` 生成（`src/settings.rs:216-227`） | 否（`.gitignore` 忽略 `/config/`） |
| `data/members.json` | 成员缓存，`POST /api/members/refresh` 与每日调度写入（`src/api/member_routes.rs:80-88`） | 否（`.gitignore` 忽略 `/data/`） |
| `data/members.seed.json` | 真实名单（gitignored，可选）；读取顺序 seed→example→空（见 F8） | 否 |
| `data/members.example.json` | 入库占位成员（`成员01`…`成员05`） | 是 |
| `config.example.json` | 配置模板/默认值来源（`src/settings.rs:183-186`） | 是 |
| `simulation-corpus/**` | 回归语料（只读资产，不得改，`docs/AGENT-RULES.md:11`） | 部分（`real-*/` 被忽略） |

### 7.2 事件模型

- `EventEnvelope`（`src/domain/event.rs:39-51`）：`event_id, round_id, group_id, user_id, raw_message_id, event_type, effective_at, sequence, payload, status`。
- `DomainEvent`（`#[serde(tag="event_type")]`，`src/domain/event.rs:7-37`）变体：
  `ClaimCreated`、`ClaimCancelled`、`ClaimModified`、`AdminAllocationAdjusted`、`AdminSlotLocked`、`AdminSlotUnlocked`、`DiscountRulesSet`、`RoundClosed`、`RoundOpened`、`ParseOverride`。
- 当前新栈**只会产生** `ClaimCreated` / `ClaimCancelled`（`src/parser/validation.rs:144,193`）；其余变体虽在重放/引擎中被处理，但无入站路径（见 §10）。
- 排序：`compare_event_order` = `effective_at ASC → sequence ASC`（`src/domain/event.rs:188-191`）。

### 7.3 `AllocationSnapshot` 关键字段

`src/domain/snapshot.rs:7-15`：

- `round_id`（字符串，新类型 `RoundId` 序列化为字符串）
- `version`（i64；新栈 = 已应用事件数，`src/llm/pipeline.rs:430`）
- `generated_at`（RFC3339）
- `item_allocations: Vec<ItemAllocation>`（`src/domain/allocation.rs:103-113`）：`item_id, item_name, kind, variant_id, boxes, singles, waiting`
  - `boxes[].slots[]`（`SlotAllocation`，`src/domain/allocation.rs:26-36`）：`slot_index, user_id, claim_id, claim_line_index, status, slot_policy, segment_id, lock_reason`
  - `singles[]`：`user_id, claim_id, item_id, quantity, unit_price`
  - `waiting[]`：`user_id, claim_id, item_id, quantity, claim_type, priority_level`
- `user_summaries: Vec<UserAllocationSummary>`：`user_id, display_name`（**新栈恒为空串**，`src/engine/allocation_engine.rs:166`）, `items[]`
- `warnings: Vec<AllocationWarning>`（新栈恒为空，`src/engine/allocation_engine.rs:92`）
- 另定义 `PublicSnapshot`（`src/domain/snapshot.rs:24-95`）供发布器使用（`to_public`，`:97-133`），但新栈未发布。

---

## 8. 测试矩阵

### 8.1 Rust 单测（`cargo test`）

共 **52** 个（按源码 `#[test]` / `#[tokio::test]` 计数）：

| 文件 | 数量 | 覆盖点 |
|---|---|---|
| `src/gateway/onebot.rs` | 12 | 路由（白名单/非白名单/非 message/空消息）、消息规范化（段/CQ/转义）、昵称清洗（备注/代理/全角）、身份、字段映射 |
| `src/gateway/ws_server.rs` | 8 | `reply_enabled` 强制（拒绝 `send_*` / 未授权动作 / 白名单放行）、无客户端报错、echo 往返、status 形状 |
| `src/gateway/action.rs` | 1 | 只读动作无客户端时失败 |
| `src/llm/pipeline.rs` | 8 | 规则排谷成功、LLM 排谷成功、非排谷忽略、歧义确认、时段拒绝、预存优先排序、LLM 失败回退、无回退拒绝 |
| `src/settings.rs` | 2 | 优先时段 end 独占、预存用户候选匹配 |
| `src/api/config_routes.rs` | 2 | 409 映射、500 映射 |
| `src/api/display_routes.rs` | 4 | 增量 diff（新填/不变/清空/移除格） |
| `src/api/member_routes.rs` | 2 | 成员解析（数组/对象）、example 占位名 |
| `src/api/sim_routes.rs` | 1 | 身份存取与列出 |
| `src/api/mod.rs` | 2 | 默认 web 目录、CORS 构建 |
| `src/tests/replay_helpers.rs` | 10 | 既有引擎/重放辅助（`src/tests/replay_helpers.rs`） |

### 8.2 Node e2e（`tests/e2e/sim.mjs`）

- 运行：`node tests/e2e/sim.mjs` / `npm run test:e2e` / `pwsh tests/e2e/run.ps1`（`tests/e2e/README.md:16-24`）。
- 脚本自建：`cargo build` → 临时配置（`bind=127.0.0.1:0`、`reply_enabled=false`、`llm.enabled=false`、固定优先窗口）拉起 `run` → Chromium 驱动 `/sim` → 断言（`tests/e2e/sim.mjs:51-98`）。
- **8 个用例**（每例前 `POST /api/sim/reset`，`tests/e2e/sim.mjs:257-386`）：
  1. 常规排谷：`成员01`（预存）+ `排 通行证 结城理 1` → `Applied`，`pass_sp/v_jcl` 首格为 `成员01`；
  2. 非排谷忽略：`今天天气不错` → `Ignored`；
  3. 时段拒绝：非预存 `成员02` + 偏移落在窗口内 → `Rejected`，原因含「预存/优先时段」；
  4. 预存优先：非预存 `成员02` 先排、预存 `成员01` 后排 → 首格变为 `成员01`；
  5. `/sim` 静态资源 200：`/sim`、`/common.js`、`/sim.js`、`/display.css` 均 200；
  6. `PUT /api/config` 冲突 409：陈旧 `revision` → `409 stale_revision`；
  7. `/api/members` 回退 example：seed 缺失时 `source=example` 且 5 个占位成员；
  8. `remote` 数据源：本地静态服务提供 `rounds/<id>/current`（无 `.json`），`/display?source=remote` 渲染。
- 断言以 HTTP API 为主、页面表格/转录为辅（`tests/e2e/README.md:62`）。

### 8.3 `simulation-corpus` 回归

- **确定性重放**：`cargo run -- simulate --round-config … --queue … --out …`（`simulation-corpus/VERIFICATION.public.md`）。
- **real-samples**：`sample1`、`sample2`，逐槽对比 `expected_allocation.json`（`simulation-corpus/real-samples/verify-samples.mjs`；`docs/TASKS.md:92` 记录「real-samples 逐格回归 ALL PASS」）。
- **real-xlsx**：两个真实团（`月行水上`、`辉夜姬`）的 xlsx → fixtures 重建（`simulation-corpus/real-xlsx/`）。
- **多 agent 语料**：`agent-a-normal`、`agent-b-box-tail`、`agent-c-cancel-fund`、`agent-d-adversarial`（`simulation-corpus/VERIFICATION.public.md`）。
- **已修复缺陷**：包盒未识别、购物金无优先通道、带商品撤销误判、FullBox 数量、包尾超规、数量 0/超 99、闲聊误拒、version 恒 1（`simulation-corpus/VERIFICATION.public.md`）。

### 8.4 覆盖点与未覆盖点

**已覆盖**：路由白名单/drop、消息规范化、昵称清洗与代理、规则与 LLM（mock）两条解析路径、回退、校验（歧义/置信度/数量）、权限时段与预存排序、快照增量 diff、config 409/500、e2e 八场景。

**未覆盖**：
- 真实 NapCat 接入链路（无 WS 集成测试；仅单元 mock echo）；
- LLM 真实网络调用（全部 mock/`enabled=false`）；
- 包盒 / 包尾 / 单领 / 撤销在**新栈 Pipeline** 中的端到端断言（仅在 `simulate` 旧引擎语料中覆盖）；
- 成员拉取成功路径（依赖 NapCat 在线，无自动测试）；
- `PUT /api/config` 成功路径与热载竞态；
- 远程（R2/Pages）数据源与发布；
- `AdminAllocationAdjusted` 等管理事件的重放（无入站路径）。

---

## 9. 部署

### 9.1 本地

```powershell
cargo run -- run
# 展示 http://127.0.0.1:21081/  ·  管理 http://127.0.0.1:21081/admin  ·  模拟 http://127.0.0.1:21081/sim
node tests/e2e/sim.mjs
```

- 数据落 `config/app.json`、`data/members.json`（`src/main.rs:82-117`）。
- `gateway.bind` 默认 `192.168.100.2:9801`；若该地址不可用，Gateway 每 2s 重试绑定并记录 `last_error`，**HTTP API 不受影响**（`src/gateway/ws_server.rs:46-72`；`/api/gateway/status` 可查）。

### 9.2 将来 Cloudflare（R2 快照 + Pages 静态展示）——**未接线**

- **接口已定义**：`SnapshotPublisher` trait，`publish_current(round_id, snapshot)` / `publish_versioned(round_id, version, snapshot)`（`src/publisher/r2_publisher.rs:7-11`）。
  - `R2Publisher`：对象键 `rounds/{round_id}/current.json`、`rounds/{round_id}/snapshots/{version}.json`；但 `client: None`（`src/publisher/r2_publisher.rs:13-25,29-63`）→ 无 S3 客户端时直接返回 `Ok(())` 不写入。
  - `LocalPublisher`：写本地 `rounds/{round_id}/current.json` 与 `rounds/{round_id}/snapshots/{version}.json`（`src/publisher/local_publisher.rs:20-42`）。
- **前端远程适配器**：`web/common.js:841-847` 依次尝试 `<remote_base>/rounds/<round_id>/current`、`<remote_base>/current`（**无 `.json` 后缀**）；`display.data_source=remote` 时启用（`web/display.js:265-287`）。
- **未接线点**：`run_gateway_stack` 不构造任何 publisher，也不在快照变更后调用发布（`src/main.rs:82-117`）；`R2Publisher` 无凭据注入；`PublicSnapshot.to_public` 未被调用。远程数据契约与本地 `/api/display` 的 `AllocationSnapshot` **未统一**（remote 预期 `PublicSnapshot`/`items`，local 为 `item_allocations`，前端 `normalizeBoard` 两者兼容，但 `messages`/`who_whats`/`changed` 远程缺失，`web/display.js:226-254`）。

---

## 10. 已知限制与未实现

1. **`reply_enabled` 已强制，但新栈未接发送路径**：`Gateway::send_action` 对 `send_*` 仅当 `reply_enabled=true` **且** `action ∈ allowed_actions` 时放行，否则 `Err` + `warn!`（默认 `false` → 拒绝，`src/gateway/ws_server.rs:75-88`）。但 `Pipeline` 产出的 `reply` 只回给 HTTP 调用方，新栈**没有调用 `send_*` 的代码路径**；即使置 `true` 也不会自动发消息。
2. **R2 发布未接线**：`R2Publisher.client=None` 且无调用；`LocalPublisher` 亦未在 `run` 中构造（见 §9.2）。
3. **管理员命令仅记录不执行**：斜杠命令（管理员）返回 `Applied`「管理员命令已记录」，无实际动作（`src/llm/pipeline.rs:135-144`）；LLM 解析出的 `AdminCommand` 被校验层拒绝并提示用斜杠格式（`src/parser/validation.rs:207-209`）。
4. **`Modify`（改单）未实现**：`ParsedIntent::Modify` 被置为 `Ignored`「改单功能暂未实现」（`src/llm/pipeline.rs:188-192`）。
5. **`/api/replay` 501**：`GET /api/replay` 与 `/api/replay/*` 恒 `501 not_implemented`（`src/api/mod.rs:50-55,70-71`）。
6. **远程数据源契约待统一**：local 返回 `AllocationSnapshot`（`item_allocations`），remote 预期 `PublicSnapshot`（`items`）；`messages`/`who_whats`/`changed` 远程无对应（`src/domain/snapshot.rs:24-95`；`web/common.js:537-661`）。
7. **枚举序列化大小写不一致**：`status`/`slot_policy`/`claim_type` 输出 PascalCase（实测 `"Filled"`/`"Normal"`/`"Split"`），前端部分样式判定用小写（`web/common.js:228-235`），导致锁定/预留样式不生效。
8. **重复消息 `seq` 复用**：`Duplicate` 记录使用当前 `state.seq`（不自增，`src/llm/pipeline.rs:87-96`），与上一条消息同 `seq`；前端消息 key 为 `s<seq>`，可能复用/覆盖节点（`web/common.js:669-670`）。
9. **无持久化**：事件/快照仅在内存，进程重启即丢失（`src/llm/pipeline.rs:37-48`）；`data/` 仅存成员缓存。
10. **`ColumnLocked`（锁列）无专门语义**：按普通槽处理（`src/engine/allocation_engine.rs:190-192`；`simulation-corpus/VERIFICATION.public.md`）。
11. **`gateway.bind` 热改不重绑**：监听循环不响应配置变更（`src/gateway/ws_server.rs:46-73`）。
12. **`sim/identity` 的身份/优先级未参与处理**：仅存储；预存判定实际来自 `config.round.priority_users` 与昵称匹配（`src/api/sim_routes.rs:85-96`；`src/llm/pipeline.rs:494-499`）。
13. **`data/members.seed.json` 默认不存在**：真实名单需用户自行放置（gitignored）；未放置时 `/api/members` 回退 `data/members.example.json`（占位 `成员01`…`成员05`），`source=example`（`src/api/member_routes.rs:39-58`）。
14. **`display` 的 `changed` 仅在命中缓存时非空**：首次或 `since` 不在最近 64 个版本内返回 `[]`（`src/api/display_routes.rs:12,25-34,55-64`）。
15. **API 无鉴权**：仅绑定 `127.0.0.1`（`src/api/mod.rs:82`）；若暴露需自行加防护。
16. **结算/优惠未接入新栈**：`rebuild` 只做分配，不调用 `SettlementEngine`（`src/llm/pipeline.rs:416-432`）；`DiscountRulesSet` 无入站路径。
17. **有界队列/背压未实现**：Gateway 直接 spawn，非 DESIGN §7 描述的有界 `mpsc`（`src/gateway/ws_server.rs:267-269`）。
18. **`simulate`/`serve` 属旧引擎**：与新栈 Pipeline 是两套解析/校验代码路径（`src/simulation/**`），二者行为可能不完全一致（待确认是否需统一）。

---

## 11. 待办与变更

- **接线远程发布**：在 `run` 栈快照变更后调用 `SnapshotPublisher`（R2/Local），并统一 remote 数据契约（对齐 `PublicSnapshot` 或扩展前端适配器）。
- **回复发送路径**：如需群内回复，需在 Pipeline 侧接线 `Gateway::send_action`（当前 `reply` 仅回 HTTP；`send_*` 门禁已强制：`reply_enabled=true` 且在白名单才放行）。
- **实现或明确拒绝 `Modify`**：改单语义（`ParsedIntent::Modify`）目前仅忽略。
- **管理员命令执行**：把 `/` 命令映射为 `AdminAllocationAdjusted` 等事件并接入重放。
- **持久化事件/快照**：重启不丢状态；为 `/api/replay` 提供真实回放数据。
- **前端大小写对齐**：统一枚举序列化（snake_case）或修正前端判定，使锁定/预留样式生效。
- **重复消息 `seq` 语义**：修正 `Duplicate` 的 seq/key 处理。
- **测试补强**：新栈下的包盒/包尾/单领/撤销端到端、成员拉取成功路径、`PUT /api/config` 成功与热载竞态、WS 集成。
- **成员兜底文件化**：是否引入 `data/members.seed.json`（gitignored）替代 `data/members.example.json` 占位（当前默认无 seed 文件）。

---

> 复核建议：`cargo test`（52）、`node tests/e2e/sim.mjs`（8/8）、`node simulation-corpus/real-samples/verify-samples.mjs`（ALL PASS），以及本机 `cargo run -- run` 后按 §6 的 curl 核对响应。
