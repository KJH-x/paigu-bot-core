# 模块契约与文件所有权（MODULES）

> 配套 [DESIGN.md](./DESIGN.md) §2、[TASKS.md](./TASKS.md)。本文件汇总各模块的**职责、对外契约、文件归属**。
> 规则：子 agent **只改自己名下的文件**；跨模块改动先在 [TASKS.md](./TASKS.md) 追加「待 A0 处理」，由 A0 统一接线。

## 0. 冻结接口（跨模块契约，改动需 A0 同意）

### `src/bus.rs` — Gateway 与 Pipeline 的解耦边界

| 项 | 契约 |
|---|---|
| `IncomingEvent` | `group_id: String`、`user_id: String`、`nickname: String`（已清洗的 display）、`message_id: String`、`text: String`、`timestamp_ms: i64`、`is_admin: bool`、`raw: serde_json::Value` |
| `trait EventSink` | `async fn handle(&self, ev: IncomingEvent)`；Gateway 只依赖此 trait |
| `PipelineOutcome` | `status: String`、`detail: String`、`reply: Option<String>`、`version: i64`、`snapshot: Option<serde_json::Value>`（`serde::Serialize`） |

`status` 取值：`Applied | Duplicate | Ignored | Rejected | NeedConfirm | Error`。

### `src/settings.rs` — 配置与热载

| 项 | 契约 |
|---|---|
| `AppConfig` | `revision: u64` + `gateway` / `llm` / `round` / `display` / `members`（字段见 DESIGN §3） |
| `GatewayConfig` | `bind`、`whitelist_groups`、`heartbeat_secs`、`reply_enabled`、`allowed_actions`（**无 `require_token`**） |
| `ConfigStore` | `load(path)`、`get()`、`revision()`、`put(cfg, expected)`（陈旧 → `ConfigError::StaleRevision`）、`reload()`、`spawn_watch()`（notify，300ms 去抖） |
| `default_config()` | 由内嵌 `config.example.json` 反序列化 |
| 纯函数 | `in_priority_window(window, timestamp_ms)`（end 独占）、`is_priority_user(users, candidates)`（单一真源，`simulation/verifier.rs` 复用） |

## 1. 归属总表

| 路径 | 内容 | 归属 |
|---|---|---|
| `src/main.rs`、`src/app_state.rs`、各 `mod.rs`、`Cargo.toml` | 装配与接线 | **A0（主）** |
| `src/bus.rs`、`src/settings.rs` | 冻结接口与配置存储 | **A0** 冻结 / **A2** 维护 |
| `src/gateway/**` | OneBot 协议、路由、反向 WS、动作回包 | **A1** |
| `src/llm/**` | OpenAI 兼容客户端、排谷流水线 | **A2** |
| `src/api/**` | axum 路由与静态页服务 | **A3** |
| `web/**` | 静态前端（display/admin/sim/replay） | **A4** |
| `tests/e2e/**` | Node `.mjs` Playwright e2e | **A5** |
| `src/engine/**`、`src/replay/**`、`src/parser/**`、`src/simulation/**`、`src/domain/**` | 既有确定性引擎 | 主（改动需 A0 同意） |

## 2. Gateway（`src/gateway/**`，A1）

- `mod.rs`：模块导出；`Gateway` 类型。
- `onebot.rs`：
  - 类型 `RouteMessageEvent`（`post_type/message_type/self_id/user_id/group_id/message_id/raw_message/message/sender`）、`RoutePolicy { whitelist_groups }`、`RouteKind { Drop, Message }`。
  - `decide_route(ev, policy) -> RouteKind`：非 `message` / 非 `group` / 非白名单 / 空文本 → `Drop`。
  - `normalize_message`：CQ 码/消息段 → 纯文本；`clean_nickname`：全角归一 + 去括号备注 + 代理写法 `A(代B)`（**昵称清洗的单一真源**，Pipeline 复用）；`parse_identity`、`to_incoming_event`。
- `ws_server.rs`：`Gateway::new(cfg, sink)`、`run()`（绑定 `gateway.bind`，失败每 2s 重试）、`send_action(action, params)`、`status()`。
  - **安全**：`send_*` 仅当 `reply_enabled == true` **且** `action ∈ allowed_actions` 时放行，否则 `Err` + `warn!`；非 `send_*` 动作须在 `allowed_actions` 内。
- `action.rs`：只读动作封装（`get_group_member_list` 等）。

## 3. LLM + Pipeline（`src/llm/**`，A2）

- `mod.rs`：`truncate(text, max)`（日志/错误信息截断，单一真源）。
- `client.rs`：`trait LlmClient` + `OpenAiClient`（`chat/completions`，`base_url/model/api_key_env/timeout/max_tokens/temperature`，JSON 模式，超时/重试）。
- `prompt.rs`：`build_system_prompt(cfg)` / `build_user_prompt(ev)`。
- `pipeline.rs`：`Pipeline::new(cfg)` / `new_with_client(cfg, llm)`；`process(ev) -> PipelineOutcome`；`impl EventSink for Pipeline`。
  - 流程：幂等（`group_id::message_id`）→ 空文本忽略 → `/` 管理员命令 → 规则快路径（`RuleParser`，`confidence >= 0.9`）→ LLM（失败按 `fallback_to_rules` 回退）→ `EventValidator` → 权限（时段/预存）→ 写事件 → `rebuild_allocation_snapshot` → 回复文本。
  - 状态：内存 `events/messages/seen/eligibilities/display/identity/seq/version/snapshot`（**无持久化**）。

## 4. HTTP API（`src/api/**`，A3）

- `mod.rs`：`ApiState { cfg, pipeline, gateway }`、`web_dir()`（`PAIGU_WEB_DIR`，默认 `web`）、`cors_layer()`、`build_router(state)`、`serve(state, port)`（`127.0.0.1:port`）。
  - 路由：`/api/config`、`/api/board`、`/api/display`、`/api/messages`、`/api/sim/*`、`/api/members(/refresh)`、`/api/gateway/status`；静态页 `/`、`/admin`、`/sim`、`/replay`；`/web/*` 与 `fallback_service` 回退到 `web/`。
  - `/api/replay`、`/api/replay/*`：**恒返回 `501 {"error":"not_implemented"}`**（桩，未接线）。
- `config_routes.rs` / `board_routes.rs` / `display_routes.rs` / `sim_routes.rs` / `member_routes.rs`：见 DESIGN §5 契约。
- 注意：`src/api/routes.rs`、`admin_routes.rs`、`public_routes.rs`、`webhook_routes.rs` 属**旧栈**路由，不在新栈装配内。

## 5. 前端（`web/**`，A4）

- 纯 vanilla JS、无构建、无框架；`display.html|js`、`admin.html|js`、`sim.html|js`、`replay.html|js|css`、`display.css`、`index.html`。
- `common.js`：API 封装、数据归一化（`normalizeBoard` / `normalizeChanged`）、keyed diff 渲染器（`createBoardRenderer`）、smart-scroll、成员归一化与内置占位子集、`resolveRemoteCandidates`。
- 数据源适配：`local`（同源 API）/ `remote`（静态快照，取 `<base>/rounds/<round_id>/current`，**无 `.json` 后缀**）。
- 约束：只依赖 DESIGN §5 的 HTTP 契约；不得改 `src/**`、`simulation-corpus/**`。

## 6. 端到端测试（`tests/e2e/**`，A5）

- `sim.mjs`：脚本自带 `cargo build` + 临时配置（`bind=127.0.0.1:0`、`reply_enabled=false`、`llm.enabled=false`、固定优先窗口）拉起 `run`，用 Chromium 驱动 `/sim` 并断言；全程只打本地 API/页面。
- `run.ps1`：一键运行；`README.md`：运行方式与用例说明。

## 7. 旧栈弃用清单（不属新栈，勿在其上新增功能）

| 路径 | 说明 |
|---|---|
| `src/config.rs` | 旧栈 `Config::from_env()`（`DATABASE_URL` 等），仅 `main.rs` 的**未识别子命令回退**使用，已弃用 |
| `src/inbound/**`、`src/ws/**` | 旧栈入站/WS 服务器（新栈用 `src/gateway/**`） |
| `src/services/**`、`src/repo/**` | 旧栈服务与 PostgreSQL 数据访问层 |
| `src/api/routes.rs` + `admin/public/webhook_routes.rs` | 旧栈路由（新栈用 `src/api/mod.rs`） |
| `src/publisher/**` | `R2Publisher` / `LocalPublisher` 已实现但 `run` 栈未构造、未调用 |
| `migrations/**` | 旧栈数据库迁移（新栈不需要 PostgreSQL） |
| `viewer/`（空目录） | 旧重放步进查看器，已合并进 `web/replay`；`src/simulation/**` 保留（`simulate`/`serve` 使用） |

> 新增依赖须在 [TASKS.md](./TASKS.md) 说明并由 A0 加入 `Cargo.toml`。
