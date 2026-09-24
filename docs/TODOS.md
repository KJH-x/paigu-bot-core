# 工程待办（TODOS）

> 每条：ID / 优先级(P0>P1>P2) / 状态 / 依赖 / 验收。
> 状态：`todo` · `doing` · `blocked` · `done`。决策依据见 [DECISIONS.md](./DECISIONS.md)。
> 领任务前先读 [AGENT-RULES.md](./AGENT-RULES.md) 与 [MODULES.md](./MODULES.md)（文件所有权）。

## 已完成（Wave 1–4，2026-09-17）

| ID | 任务 | 验收 |
|---|---|---|
| T-01 | **减均重构（A-1）**：`B`=Σ各包实付价、`C`=无折扣商品总价、`D=C−B+G`，按「件×标价」加权分摊；整数分 + 残差校正；校验 `Σfinal + G = B` | ✅ 单测含校验式/边界/逐件精度 |
| T-02 | **成团/特典认购（A-2）**：`granted=min(claims,P)`；不足整盒不成团（无包尾）；`G=Σgranted×价` | ✅ 月行水上-更新 **G=¥144** |
| T-03 | **标价表入商品目录（A-4）**：`round.items[]`+`variants[]` 增标价/盒件数/件数并透传；结算/planner 从目录取价（`to_unit_prices`）；admin 页可编辑 | ✅ 结城理=5000、虎狼丸=2500 |
| T-04 | **`reply_enabled` 热切换（B-1）**：`POST /api/config/reply` + admin 复选框；默认关闭 | ✅ 关闭时 `send_*` 必被拦截 |
| T-05 | **共享 `MessageLog`（C-5/T-05）**：`main.rs` 注入，Gateway/Pipeline/API 共用；`EventSink::messages()` | ✅ 单一实例 |
| T-06 | **单一 JSON 快照（C-2）**：`SnapshotBundle::{export_file,import_file}` + `format=file` + 路径自动识别 | ✅ 往返测试 |
| T-07 | **原始事件保留（C-3）**：`data/events/<round>.jsonl`（WS 由 Gateway 落盘、API 由 Pipeline 落盘）+ `GET /api/events` | ✅ |
| T-08 | **细粒度方法（C-5）**：`MessageLog::{query,update,delete}`；`delete` 已接线到 `/api/messages/:seq` | ✅ |
| T-09 | **管理员命令落地（D-1）**：`gateway.admin_commands_enabled` 热开关；`/开团 · /锁位 · /结团 · /状态 · /导出` 生效（锁定后拒绝排/撤/改） | ✅ 单测 |
| T-10 | **改单（D-2）**：`Modify` → 撤销本人该商品既有认购 + 重新认购 | ✅ 单测（自助改单） |
| T-11 | **子命令统一（D-4）**：`serve` 并入新栈；未知子命令按 `run` 启动 | ✅ |
| T-12 | **删除旧栈（C-4）**：`dev` 分支删除 `config/app_state/repo/services/ws/publisher/旧 api/inbound/chat_server` + 移除 `sqlx/csv/aws-*/figment/socket2`；验证后合并 `master` | ✅ 135→138 测试全绿、e2e 11/11、real-samples/real-xlsx ALL PASS |
| W1-5 | **planner 聚合（A-3）**：原子去 `owner`、标价取目录 canonical、merge key 去价 | ✅ |
| W1-6 | **排包完成校验**：`check_completeness` + `/api/settlement/completeness`；evaluate 遇未完成拒绝计算 | ✅ |

## 已完成（Wave G / SPEC-UPDATE U1–U9，2026-09-24）

| ID | 任务 | 验收 |
|---|---|---|
| G-01 | **轮次库（U3）**：`active_round_id` + `data/rounds/<id>.json`；`GET/POST /api/rounds`、`activate`（`continue`/`fresh`/`replay`）、`check`、`DELETE`；`resolve_active` 热载 | ✅ `settings/rounds` + `services/rounds` 单测 |
| G-02 | **导航/流程语义（U1）**：Stepper 高亮=当前模块 + 弱阶段进度；`/round`（开团）与 `/settings` 分流 | ✅ `GET /api/workflow.phases` + 前端 shell |
| G-03 | **商品模型/编辑器（U6）**：4 种类、`class` 自动推导、变体 A±B=C 三格联动、别名本地词切+锁定+冲突校验、末尾虚线卡/缝隙「+」、单领多行、整盒独立种类、消息日志折叠面板 | ✅ `web/round.*`；`check` 覆盖跨商品冲突（`adjust_cents` 落库见 W-G2-01） |
| G-04 | **成员 CN（U4）**：`cn_overrides`；回退 CN→归一化昵称→user_id；`/api/members` 附 `cn/resolved`；展示层 who_whats 覆盖 | ✅ `services/members` 单测 |
| G-05 | **first-match + 失败澄清（U7）**：目录序第一个可拼团；全 fail→LLM 澄清→`ParseOverride` 持久化 + 重放消费 | ✅ `llm/pipeline/tests` 实时==重放 |
| G-06 | **整盒/包尾（U8）**：整盒→单领队列；包尾锁定列=最大列序+1；结算强制成盒 + 自动滑入 | ✅ `domain/allocation` + `allocation_engine` 单测 |
| G-07 | **时间窗相对日（U5）**：`周几+今天/明天/后天/X天前后` | ✅ 前端 `shell.js` |

## 本轮遗留（Wave G2，2026-09-24）

> 正文已按现行口径更新；以下为实现与文档的**已知差距/待办**（P1>P2）。

| ID | 优先级 | 状态 | 任务 | 验收 |
|---|---|---|---|---|
| W-G2-01 | P1 | **done** ✅ | **`VariantConfig.adjust_cents` 落库**：Rust 声明该字段 + `to_unit_prices`/结算取价接线；当前仅 `web/round.js` 前端契约（保存时被 serde 忽略、不落 `data/rounds/*.json`） | `/round` 保存 A/B 后重载仍存在，结算见调后价 |
| W-G2-02 | P1 | **done** ✅ | **`POST /api/items/suggest-aliases` 后端实现**（LLM 别名建议，返回 `{suggestions:[{item_id,verdict?,aliases?}]}`）；前端已接入并对 404/405/501 兜底 | 按钮返回建议/「是最佳」 |
| W-G2-03 | P2 | **done** ✅ | **`GET /round`、`GET /settings` 短路由**（如需）：当前仅静态文件 `/round.html`、`/settings.html` | 短路由可直接访问 |
| W-G2-04 | P2 | **done** ✅ | **清理 `box_size`/`variants[].pieces` 残留**：Rust `ItemConfig.box_size`/`VariantConfig.pieces` 字段、`to_items` 透传、`config.example.json` 旧值 | 代码/配置不再含该字段且回归全绿 |
| W-G2-05 | P2 | todo | **重放快照的 CN**：`replay_engine::to_allocation_snapshot`（及 `replay/session`）产出的 `user_summaries.display_name` 未走 CN | 重放人名与 `/api/display` 一致 |
| W-G2-06 | P2 | todo | **`SlotPolicy::ColumnLocked` 语义未定义**：`allocation_engine` 当前按 `normal` 处理；需明确「锁列」含义或移除 | 语义有文档与单测 |
| W-G2-07 | P2 | todo | **`check` 跨商品规则细化**：变体名跨商品重名（角色名）当前不报错、`class_derived_mismatch` 为 warn；按需分级/扩展 | 规则有单测 |

## 待办

### P1

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-13 | done | **阶段权限接入实时链路**：实时+重放均按 `phase_at` 拒绝越权；A/B 分类由 `class` **自动推导**（有变体⇒A/无变体⇒B，`RoundSettings::item_class`）+ `/round` 编辑器设置 `kind` | A-5 | ✅ 越权实时==重放单测；分类自动推导单测 |
| T-14 | todo | **管理员改单目标语法**：`/改单 <目标> ...`（管理员改群内任意指定人）；需把目标昵称解析为 user_id（可用 `state.display/identity`） | D-2 | 管理员改他单用例 |
| T-15 | todo | **`MessageLog::update` 接线**：`PUT /api/messages/:seq` 仍走 read_all+replace_all，改用细粒度 `update` | — | 不再全量重写 |
| T-16 | todo | **快照导出/导入前端入口**：`web/admin` 按钮 + 下载（含 `format=file`） | T-06 | admin 可操作 |
| T-17 | todo | **`/api/replay` diff 可视化**（`web/replay`） | — | diff 可见 |
| T-18 | done | **成员 user_id 映射**（NapCat 拉取写入缓存）：refresh 已写缓存（`data/members.json`），映射实现完成；待实机验证 | B-2 | 映射非空 |

### P2

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-19 | todo | **每日 19:00 拉取验证 + 失败告警** | B-2 | 缓存更新/告警 |
| T-20 | todo | **真实昵称零残留守护脚本**（纳入回归） | — | 命中即失败 |
| T-21 | todo | **`FUNCTIONAL.md` 引用改函数名**（现行号易漂移） | — | 不再依赖行号 |
| T-22 | todo | **结算配置样例入 `config.example.json`** | — | 样例可加载 |
| T-23 | todo | **入站有界队列/背压**（DESIGN §7 的 mpsc） | — | 压测不丢消息 |
| T-24 | todo | **`simulate` 迁移到新栈**（当前保留旧 verifier 以支撑 real-samples/real-xlsx 回归；迁移需保持逐格一致） | T-11 | 同输入同结果 |
| T-25 | todo | **拆大文件**：`llm/pipeline.rs`(1300+)、`simulation/verifier.rs`(700+) | — | 职责清晰 |
| T-26 | todo | **补单测**：`domain/**`、`engine/replay`、`api/board_routes` | — | 覆盖率提升 |
| T-27 | todo | **远程展示接线**（C-1 暂不部署；`domain::snapshot::Public*` 视图模型已保留） | — | 待用户开启 |

## 技术债（2026-09-23 审查 → 已全部处理）

> 来源：4 份代码审查（结算栈 / 重复策略 / 重放 / 架构分层）。**D-01…D-09 已于同日全部完成**（`cargo test` 154 passed / 0 warning / e2e 11-11 / real-samples·real-xlsx ALL PASS）。

| # | 状态 | 处置结果 |
|---|---|---|
| D-01 | ✅ | **统一结算真源**：`SettlementEngine::settle` 改为调用 `settlement::evaluate` 并映射回 `SettlementSnapshot`（适配器路线；对外 DTO/JSON 不变）。`src/engine/settlement_engine.rs:39`、`allocation_to_order_table` |
| D-02 | ✅ | **抽出共享策略模块** `src/parser/policy.rs`：群/成员白名单、优先时段/优先用户、`phase_label`/`phase_rejection`；`llm/pipeline` 与 `replay/session` 共用。新增对照测试 `realtime_and_replay_phase_rejection_match`、`realtime_and_replay_whitelist_decisions_match` |
| D-03 | ✅ | **删除死重放栈**：`ReplayService`/`rebuild_snapshot`/`collect_discount_rules`；保留 `rebuild_allocation_snapshot`/`collect_effective_claims`/`describe_event`（签名不变） |
| D-04 | ✅ | **拆分** `src/llm/pipeline.rs` → `pipeline/{mod,state,llm_parse,admin,export,tests}.rs` |
| D-05 | ✅ | **`clean_nickname` 下沉**至 `src/parser/normalize.rs`；`gateway` 之外不再依赖 `gateway::onebot` |
| D-06 | ✅ | **删除** `src/error.rs` 与 `thiserror` 依赖（领域层无失败路径），应用层统一 `anyhow` |
| D-07 | ✅ | **测试放置统一**（约定见 [AGENTS.md](../AGENTS.md) §4）：测试模块 ≥120 行 → 目录化 `foo/mod.rs` + `foo/tests.rs`；<120 行内联。已落地 `allocation_engine`/`settlement_engine`/`onebot`/`ws_server`/`session`/`settings`/`planner`/`settlement`/`messages`/`api`/`llm/pipeline` 等 |
| D-08 | ✅ | **消息三件套整合**：删除 `MessageStore` trait，`MessageLog` 成为唯一对外存储 API（`JsonlMessageStore` 降为内部句柄） |
| D-09 | ✅ | **API 薄层化**：新增 `src/services/**`（settlement/display/messages/members/replay/snapshot）；handler 仅做请求→服务→响应映射，**HTTP 契约不变**（e2e 11/11） |

## 关键提交

| 提交 | 内容 |
|---|---|
| `de06d22` | T5 planner + T6 结算 UI |
| `de53893` | **Wave 1**：减均 v2 + 成团/特典 + 标价表 + 月行水上-更新夹具 |
| `15a4860` | W1-5/W1-6：planner 聚合 + 排包完成校验 |
| `efc3354` | **Wave 2**：共享 MessageLog + 原始事件 + 细粒度方法 |
| `07f8ac0` | Wave 2：单 JSON 快照 + `/api/events` |
| `6ccf531` / `449347c` | **Wave 4**：删除旧栈（dev → master 合并） |
| `45ebc0d` | **Wave 3**：reply/admin 热开关 + 管理员命令执行 + 改单 + admin 标价 UI |
