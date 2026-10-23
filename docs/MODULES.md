# 模块契约与文件所有权（MODULES）

B1（2026-10-23，主代理统一接线）：`session/incremental.rs`只持有逐组输出备忘/签名与入口，`session/mod.rs`的同一compute/evaluate承担全量与后缀执行，不复制分配器；`ParseOverrides::input_for`封装完整有效覆盖键。`pipeline/live.rs`持有受共同writer保护的实时推进/完整收据/部分投影不落盘边界，`State`持有随轮parked的memo/cut；轮次fresh和测试reset在提交前准备记录身份，不删源。原`MessageLog::read_all`迁移式包装现仅测试使用，生产合并路径改严格只读源；内部query的既有迁移合同不变。普通非合并持续发布仍待B2，不把本批称全部完成。

UP-02基础（2026-10-23，主代理统一接线）：`replay/computed`只管私有完整上下文、内部字段封包、校验/有界加载/可丢弃原子写；仍调用`session::replay_from_journal`，不另造解析/分配器。`MessageLog::computed_path`校验轮次及私有根；`build.rs`以全部Rust源/Cargo文件计算源码依赖指纹，复用既有sha2作构建依赖，无新下载包。启动/冷轮恢复、事务/配置/导入提交检查点与显式replay边界由主代理统一装配；普通入站及尾组增量仍见工程计划§16的B批，不能把基础缓存当完整统一栈完成。

T-16 / T-25（2026-10-23，主代理统一接线）：`services/snapshot/mod.rs`持有严格完成记录/完整bundle构建与路径导出，`restore.rs`持有受保护预检/应用，`transfer.rs`持有浏览器文件限额与上下文，`api/snapshot_transfer_routes.rs`仅做私有传输头/Origin和HTTP映射；`web/private-backup.js`是系统数据维护的唯一文件交互控制器，复用既有草稿保护，不在admin另造编辑器。旧斜杠导出在§12.1尚未迁移，现由§15内部`export_completed_locked`复用完成动作构建/写文件（HTTP仍须idle），`pipeline/export.rs`不再另造宽松封包；`MessageLog::snapshot_dir`集中私有默认根/显式覆盖，检查入口与桌面均隔离该路径。

T-17（2026-10-23，已验收）：主代理统一修改`services/replay.rs`及配置/轮次/人工事务/恢复中旧remember调用。诊断只在当前实例共同writer下计算，不维护进程全局最后预览缓存；显式预览、诊断和实时应用分开。`llm/pipeline/replay_diff_tests.rs`按真实共享状态/HTTP验证，不新增业务权限或前端真源。

W-G2-07 / T-25（2026-10-23，主代理统一审计）：`services/rounds/mod.rs`保留轮次创建/设置/激活/删除/文件IO；`check.rs`纯结构/类别诊断，从原rounds.rs移动函数体而非复制真源；原超过120行内联测试移`tests.rs`，有限来源组合/first-match/不改目录性质在`catalog_tests.rs`。`rounds::category_issues`仍为原crate导出路径，外部handler/配置校验不改调用，源码链接同步。

> 配套 [DESIGN.md](./DESIGN.md) §2、[TASKS.md](./TASKS.md)。本文件汇总各模块的**职责、对外契约、文件归属**。
> 规则：子 agent **只改自己名下的文件**；跨模块改动先在 [TASKS.md](./TASKS.md) 追加「待 A0 处理」，由 A0 统一接线。

> 当前专项（2026-10-18）：主代理按 TASKS/消息计划统一接线，无子代理。`src/services/configuration.rs` 为所有配置更新/热载与确定性投影的应用边界；`src/settings/commit.rs` 为可恢复配置/轮次落盘；`src/messages/replacement.rs` 为私有备份替换恢复；`src/persistence.rs` 提供同步原子文件替换。所有运行时消息/配置/锁/导入写操作使用同一 MessageLog writer；下方历史签名以现行 INTERFACES §8.9 为准。

2026-10-22代排：`settings/targets.rs`为源群受约束的一次性确定性成员索引；`pipeline/proxy.rs`负责分段与归属，独立真实actor/资格主体。bus与MessageRecord可选delegation不改旧None指纹；session共用权限/冻结动作。展示/复盘汇总携user_id作行身份，不再按显示名重猜CN。合同见INTERFACES §8.13；不授予管理/阶段豁免。

同日T-14接线：`pipeline/target_modify.rs`只负责命令授权/唯一目标和规范化视图，不另造分配器；body/actor/source保存在admin_modify来源，原text不改，共用understanding及session权限。`pipeline/names.rs`统一提交后的称呼更新、无动作时同代快照同步，失败/Drop/重复不提升姓名真源；不是持久computed缓存。主代理统一修改common coalesce的全角斜杠控制边界，防止命令混进同语义组，详见INTERFACES §8.14。

## 0. 冻结接口（跨模块契约，改动需 A0 同意）

### `src/bus.rs` — Gateway 与 Pipeline 的解耦边界

| 项 | 契约 |
|---|---|
| `IncomingEvent` | `group_id: String`、`user_id: String`、`nickname: String`（已清洗的 display）、`message_id: String`、`text: String`、`timestamp_ms: i64`、`is_admin: bool`、`raw: serde_json::Value` |
| `trait EventSink` | `handle(ev)`；持久入站用 `deliver(binding,ev) -> Result<Delivery>`，生产Pipeline在共同writer内重验绑定；Gateway只依赖此trait |
| `PipelineOutcome` | `status: String`、`detail: String`、`reply: Option<String>`、`version: i64`、`snapshot: Option<serde_json::Value>`（`serde::Serialize`） |

`status` 取值：`Applied | Duplicate | Ignored | Rejected | NeedConfirm | Error | Merged`（`Merged` = 合并组成员行，2026-10-13）。

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
| `src/main.rs`、各 `mod.rs`、`src/error.rs`、`Cargo.toml` | 装配与接线 | **A0（主）** |
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
- `ws_server/mod.rs`：Gateway连接/echo/只读动作；`intake.rs` 为本地持久帧、容量1唤醒和单worker，按轮次绑定、失败暂停；`status()` / `retry_intake()` 暴露本地管理状态。完整约束见 [持久入站](./SPEC-DURABLE-INTAKE.md)。
  - **安全**：`send_*` 仅当 `reply_enabled == true` **且** `action ∈ allowed_actions` 时放行，否则 `Err` + `warn!`；非 `send_*` 动作须在 `allowed_actions` 内。
- `action.rs`：只读动作封装（`get_group_member_list` 等）。

## 3. LLM + Pipeline（`src/llm/**`，A2）

成员拉取的进程内健康/日历调度另在 `services/member_pull.rs`，由ApiState持有Monitor；不是Gateway或成员缓存文件里的另一份真源。服务层共用手动/每日调用，监控只存通用成败及时间，不存名单/请求载荷；原缓存安装仍由 `services/members.rs` 完成。

- `mod.rs`：`truncate(text, max)`（日志/错误信息截断，单一真源）。
- `client.rs`：`trait LlmClient` + `OpenAiClient`（`chat/completions`，`base_url/model/api_key_env/timeout/max_tokens/temperature`，JSON 模式，超时/重试）。
- `prompt.rs`：`build_system_prompt(cfg)` / `build_user_prompt(ev)`。
- `pipeline.rs`：`Pipeline::new(cfg)` / `new_with_client(cfg, llm)`；`process(ev) -> PipelineOutcome`；`impl EventSink for Pipeline`。
  - 流程：幂等（`group_id::message_id`）→ 空文本忽略 → `/` 管理员命令 → 规则快路径（`RuleParser`，`confidence >= 0.9`）→ LLM（失败按 `fallback_to_rules` 回退）→ `EventValidator` → 权限（时段/预存）→ 写事件 → `rebuild_allocation_snapshot` → 回复文本。
  - 状态：内存 `events/messages/seen/eligibilities/display/identity/seq/version/snapshot` 为投影；MessageLog保存原文/来源、冻结自动理解、人工事务及原始事件，当前会话重算/恢复。入站私有磁盘帧另有确认收据，不把内存投影当唯一真源。

## 4. HTTP API（`src/api/**`，A3）

- `mod.rs`：`ApiState { cfg, pipeline, gateway }`、`web_dir()`（`PAIGU_WEB_DIR`，默认 `web`）、`cors_layer()`、`build_router(state)`、`serve(state, port)`（`127.0.0.1:port`）。
  - 路由：`/api/config`、`/api/board`、`/api/display`、`/api/messages`、`/api/sim/*`、`/api/members(/refresh)`、`/api/gateway/status`；静态页 `/`、`/admin`、`/sim`、`/replay`；`/web/*` 与 `fallback_service` 回退到 `web/`。
  - `/api/replay`、`/api/replay/*`：逐步重放（Wave 1–4 已实现；契约见 [INTERFACES.md](./INTERFACES.md) §8）。
- `config_routes.rs` / `board_routes.rs` / `display_routes.rs` / `message_routes.rs` / `replay_routes.rs` / `settlement_routes.rs` / `sim_routes.rs` / `member_routes.rs`：见 DESIGN §5 契约。
- 注意：`src/api/routes.rs`、`admin_routes.rs`、`public_routes.rs`、`webhook_routes.rs` 属**旧栈路由，已在 C-4 删除**，不在新栈装配内。

## 5. 前端（`web/**`，A4）

- 纯 vanilla JS、无构建、无框架；`display.html|js`、`admin.html|js`、`sim.html|js`、`replay.html|js|css`、`display.css`、`index.html`。
- `common.js`：API 封装、数据归一化（`normalizeBoard` / `normalizeChanged`）、keyed diff 渲染器（`createBoardRenderer`）、smart-scroll、成员归一化与内置占位子集、`resolveRemoteCandidates`。
- 数据源适配：`local`（同源 API）/ `remote`（静态快照，取 `<base>/rounds/<round_id>/current`，**无 `.json` 后缀**）。
- 约束：只依赖 DESIGN §5 的 HTTP 契约；不得改 `src/**`、`simulation-corpus/**`。

## 6. 端到端测试（`tests/e2e/**`，A5）

- `sim.mjs`：脚本自带 `cargo build` + 临时配置（`bind=127.0.0.1:0`、`reply_enabled=false`、`llm.enabled=false`、固定优先窗口）拉起 `run`，用 Chromium 驱动 `/sim` 并断言；全程只打本地 API/页面。
- `run.ps1`：一键运行；`README.md`：运行方式与用例说明。

## 7. 旧栈弃用清单（已在 C-4 删除）

> 旧栈（PostgreSQL/sqlx + `repo`/`services`/`ws`/`publisher`/`inbound`/旧 `api` 路由/`config`/`app_state`）
> 已在 Wave 4（C-4）从 `dev` 分支删除并合并 `master`；相关 `sqlx/csv/aws-*/figment/socket2` 依赖同步移除。
> 新栈一律使用 `src/gateway/**`（接入）、`src/api/*_routes.rs`（路由）、`src/settings.rs`（配置）。

| 路径（已删除） | 原用途 |
|---|---|
| `src/config.rs`、`src/app_state.rs` | 旧栈配置（`DATABASE_URL` 等）与应用状态装配 |
| `src/inbound/**`、`src/ws/**` | 旧栈入站/WS 服务器（新栈用 `src/gateway/**`） |
| `src/services/**`、`src/repo/**` | 旧栈服务层与 PostgreSQL 数据访问层 |
| `src/api/{routes,admin_routes,public_routes,webhook_routes}.rs` | 旧栈路由（新栈用 `src/api/mod.rs`） |
| `src/publisher/**` | `R2Publisher` / `LocalPublisher`（远程发布待后续重新接线） |
| `migrations/**` | 旧栈数据库迁移（新栈不需要 PostgreSQL） |
| `src/simulation/chat_server.rs` | 旧 `serve` 聊天服务器（已并入新栈） |

> `viewer/` 为旧重放步进查看器目录（已合并进 `web/replay`）；`src/simulation/**` 保留（`simulate` 使用）。

> 新增依赖须在 [TASKS.md](./TASKS.md) 说明并由 A0 加入 `Cargo.toml`。
