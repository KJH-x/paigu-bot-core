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

## 待办

### P1

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-13 | doing | **阶段权限接入实时链路**：主体已实现（实时+重放均按 `phase_at` 拒绝越权）；仅缺 A/B 分类 UI 与来源回填（现依赖 `round.items[].class` 配置） | A-5 | 越权 Reject 的 e2e + 分类回填 |
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

## 技术债（本轮审查，2026-09-23）

> 来源：本轮 4 份代码审查（结算栈 / 重复策略 / 重放 / 架构分层）。`P0>P1>P2`；每条给出证据文件。

| # | 优先级 | 技术债 | 证据文件 |
|---|---|---|---|
| D-01 | P1 | **统一结算栈**：`settlement`（v2，`evaluate`）与 `engine::settlement_engine`（旧，`DiscountRule`）两套并存，语义重叠 | `src/settlement/{engine,model}.rs`、`src/engine/settlement_engine.rs`、`src/domain/discount.rs` |
| D-02 | P1 | **三份重复策略**：`pipeline::process` / `session::process_one` / `verifier::verify` 各有一套「规则→LLM→校验→权限」，易漂移 | `src/llm/pipeline.rs`、`src/replay/session.rs`、`src/simulation/verifier.rs` |
| D-03 | P1 | **重放引擎收敛**：`engine::replay`（内存）、`replay::replay_engine`（逐步）、`replay::session`（消息重放）三处重叠 | `src/engine/replay.rs`、`src/replay/replay_engine.rs`、`src/replay/session.rs` |
| D-04 | P1 | **拆分 `src/llm/pipeline.rs`（1300+ 行）**：与 T-25 合并；职责过多（解析/校验/权限/管理员命令/who-whats） | `src/llm/pipeline.rs` |
| D-05 | P2 | **`clean_nickname` 下沉出 `gateway`**：`llm::pipeline` 直接依赖 `gateway::onebot::clean_nickname`，形成 llm→gateway 反向依赖 | `src/gateway/onebot.rs`、`src/llm/pipeline.rs` |
| D-06 | P2 | **`error.rs` 收敛**：`AppError`/`AppResult` 与 `anyhow` 混用；`SettlementError` 为空枚举；HTTP 映射分散 | `src/error.rs`、`src/api/*_routes.rs` |
| D-07 | P2 | **测试风格统一（当前 4 种）**：inline `mod tests` / 同级 `tests.rs` / `src/tests/` / Node e2e。统一后并入 AGENTS 约定 | 见 `AGENTS.md`「测试放置」各例 |
| D-08 | P2 | **`MessageStore` / `MessageLog` / `EventSink` 三件套整合**：历史 trait 概念与现役 `MessageLog` 并存 | `src/bus.rs`、`src/messages/mod.rs` |
| D-09 | P2 | **API 薄层化**：handler 直接持有 `Pipeline`/`MessageLog`/`Gateway` 并做业务判断，宜下沉到服务层 | `src/api/{config,board,display,message,replay,settlement,sim,member}_routes.rs` |

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
