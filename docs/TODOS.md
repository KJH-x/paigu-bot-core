# 工程待办（TODOS）

> 每条：ID / 优先级(P0>P1>P2) / 状态 / 依赖 / 验收。
> 状态：`todo` · `doing` · `blocked` · `done`。决策依据见 [DECISIONS.md](./DECISIONS.md)。
> 领任务前先读 [AGENT-RULES.md](./AGENT-RULES.md) 与 [MODULES.md](./MODULES.md)（文件所有权）。

## 当前专项收口（2026-10-19）

后继执行（2026-10-21）：前项目标已签名推送 `9bb8bf2`；[工程计划](./PLAN-engineering-followup-2026-10-19.md) 区分已实现/部分/仍有效/待决策。E0/E1隐私守护、E2隔离入口已签名推送；E3共享会话迁移已通过334核心、30桌面、公开226条与两套私有只读语料，待本批签名推送；E4仍进行。扫描覆盖暂存索引＋工作副本、card/CN/别名/自定义缓存/轮次，vendor仅姓名词典匹配豁免；隐私源损坏失败而非跳过，不输出匹配值。该守护是已知来源/模式检测，不是自动匿名化或完整DLP证明；无私有来源的公开checkout只能检查通用模式与禁止路径。

消息 MP-0…6 与桌面 IA G0…G5 的实施及 MP 41＋IA 18 项已逐条补齐可重复证据，见 [验收台账](./VERIFICATION-msg-ia-2026-10-18.md)。本轮最终门禁已通过，签名交付由实际Git核验；仍未部署、未重启常驻实例。下方带日期批次记录保留原进度语境，不据历史“仍待”回滚现有功能。现有工程 todo 含已被新方案实现或替代的条目；当前目标完成后按用户授权另设目标核实，不把这些旧条目一概重新执行。

## 已完成（Wave 1–4，2026-09-17）

### 排表固定宽度（2026-10-13）

- 已完成：展示/复盘表格与列视图增加「三字宽 / 全长」，偏好跨页/标签页共享；实际字体测宽，商品内共享盒列轨道；盒色在上、锁定/预留/包尾/变化标记在下；空位同宽、超宽商品内横滚；复盘按全历史定宽、保留增量格子身份与追溯。
- 冲突检查：基于 `fb9fb53` 的干净工作区实施，保留测试轮隔离、批量注入与模拟器移除，不修改 Rust 或 API/消息处理。修正复盘列表缺少索引参数造成的重复渲染键。
- 验证：223 项 Rust 单测、17 项端到端测试通过；1920×1080、100% 下验证盒列误差 ≤1px、全长不裁切、明暗截图、跨标签同步和高亮时效；只使用隔离服务/占位夹具，未部署或重启常驻实例。

| ID | 任务 | 验收 |
|---|---|---|
| T-01 | **减均重构（A-1）**：`B`=Σ各包实付价、`C`=无折扣商品总价、`D=C−B+G`，按「件×标价」加权分摊；整数分 + 残差校正；校验 `Σfinal + G = B` | ✅ 单测含校验式/边界/逐件精度 |
| T-02 | **成团/特典认购（A-2）**：`granted=min(claims,P)`；不足整盒不成团（无包尾）；`G=Σgranted×价` | ✅ 月行水上-更新 **G=¥144** |
| T-03 | **标价表入商品目录（A-4）**：`round.items[]`+`subitems[]` 增标价/盒件数/件数并透传；结算/planner 从目录取价（`to_unit_prices`）；admin 页可编辑 | ✅ 结城理=5000、虎狼丸=2500 |
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
| G-03 | **商品模型/编辑器（U6）**：4 种类、`class` 自动推导、子项 A±B=C 三格联动、别名本地词切+锁定+冲突校验、末尾虚线卡/缝隙「+」、单领多行、整盒独立种类、消息日志折叠面板 | ✅ `web/round.*`；`check` 覆盖跨商品冲突（`adjust_cents` 落库见 W-G2-01） |
| G-04 | **成员 CN（U4）**：`cn_overrides`；回退 CN→归一化昵称→user_id；`/api/members` 附 `cn/resolved`；展示层 who_whats 覆盖 | ✅ `services/members` 单测 |
| G-05 | **first-match + 失败澄清（U7）**：目录序第一个可拼团；全 fail→LLM 澄清→`ParseOverride` 持久化 + 重放消费 | ✅ `llm/pipeline/tests` 实时==重放 |
| G-06 | **整盒/包尾（U8）**：整盒→单领队列；包尾锁定列=最大列序+1；结算强制成盒 + 自动滑入 | ✅ `domain/allocation` + `allocation_engine` 单测 |
| G-07 | **时间窗相对日（U5）**：`周几+今天/明天/后天/X天前后` | ✅ 前端 `shell.js` |

## 本轮遗留（Wave G2，2026-09-24）

> 正文已按现行口径更新；以下为实现与文档的**已知差距/待办**（P1>P2）。

| ID | 优先级 | 状态 | 任务 | 验收 |
|---|---|---|---|---|
| W-G2-01 | P1 | **done** ✅ | **`SubitemConfig.adjust_cents` 落库**：Rust 字段、往返序列化、`to_unit_prices`/结算取价均已接线 | `/round` 保存 A/B 后重载仍存在，结算见调后价 |
| W-G2-02 | P1 | **done** ✅ | **`POST /api/items/suggest-aliases` 后端实现**：LLM 别名建议，返回 `{suggestions:[{item_id,verdict?,aliases?}]}`；前端带未启用降级 | 按钮返回建议/「是最佳」 |
| W-G2-03 | P2 | **done** ✅ | **`GET /round`、`GET /settings` 短路由**：`src/api/mod.rs` 显式路由，`.html` 继续兼容 | 短路由可直接访问 |
| W-G2-04 | P2 | **done** ✅ | **清理 `box_size`/`subitems[].pieces` 配置残留**：字段、转换透传、示例旧值均已删除；领域 `Item.box_size` 是引擎运行时容量，不属配置残留 | 配置代码/样例不再含旧字段且回归全绿 |
| W-G2-05 | P2 | done | **simulate共用当前会话与CN输入**：显式夹具CN进入消息、每步/最终摘要和账单，无私有cache读取 | 2026-10-21：真实隔离Pipeline逐prefix对照、CN/别名优先与报告确定性；公开CLI也验证CN及坏cache不读取 |
| W-G2-06 | P2 | todo | **`SlotPolicy::ColumnLocked` 语义未定义**：`allocation_engine` 当前按 `normal` 处理；需明确「锁列」含义或移除 | 语义有文档与单测 |
| W-G2-07 | P2 | todo | **`check` 跨商品规则细化**：子项名跨商品重名（角色名）当前不报错、`class_derived_mismatch` 为 warn；按需分级/扩展 | 规则有单测 |

## 待办

### 桌面信息架构与功能整并（2026-10-15 计划，2026-10-19 持续实施/验收中）

- 计划入口：[PLAN-ui-ia-2026-10-15.md](./PLAN-ui-ia-2026-10-15.md)。G0 核心、七区导航、分区保存、成员独立与守卫、权限设置草稿往返已隔离回归；第十批结算复核/问题定位最终回归中，关系图和完整逐项终检尚未闭环。不是暂停，也不代表已部署。
- 建议方向：总览合并首页/管理；成员独立；消息编辑集中排谷；结算/下单同一工作区；系统设置按接入、消息理解、展示默认、数据维护分区；共用导航与连接上下文。
- 前置门禁：先修人工确认消息身份、CN 逐字编辑丢失、空商品阻断锁定；补整盒关联保存保真验证。未解决前不把 UI 搬迁算作验收通过。
- 实施与验证状态以计划 G0–G5 和固定用例为准；此条不把旧计划 §9/§11、公开部署或常驻实例操作恢复为进行中。

### 消息动作 override 与单步撤回（2026-10-16 重订，持续实施/验收中）

- 专项计划：[PLAN-message-reprocess-2026-10-15.md](./PLAN-message-reprocess-2026-10-15.md)，补充 IA 消息工作区，依赖 G0 身份修复并与 UP 唯一处理链接线；不另建入站/重算双路径。
- 已确认（DECISIONS §H）：自动处理结果先保留，人工从当前动作草稿点改并附解释，程序生成完整 override；一次保存立即按原位置重算，不再强制 LLM、不二次确认，后续撤排/改单继续执行。
- 已接受 H-7：默认复制当前动作的可点改动作句，复杂说明可显式 LLM 代填同一草稿，再手改/保存；只有代填请求模型，保存/重算不再调用，不要求人工填写 JSON。
- 手工合并：连续/不连续补充等效“主条完整动作替换 + 补充条人工拒绝”，原子保存与整笔撤回；不是独立订单联合解析。优先名单变更复用已存动作，只重验权限/逻辑重算。
- 工程门禁：稳定 ID、自动动作完整持久化（含被权限拒绝者）、原自动/人工结果分层、旧行更新版本、后续撤销引用；检查点支持撤回人工事务，但不把新消息/撤排随旧快照覆盖掉。
- 状态：已授权继续全部剩余计划，前四批已签名推送。第五批统一热载/锁/轮次/维护/备份写边界与可恢复落盘，G0 核心真实交互通过；Rust 293、e2e 22、clippy/build 及 99 条/46 商品只读语料回归通过。设置返回草稿、结算依赖提示及 IA G1–G5 仍未完成，41 项目标和 IA 18 项不得整体打勾。排撤语义见 [用户提议](./USER-PROPOSALS-2026-10-16.md)，当前证据见 [专项 §18.2](./PLAN-message-reprocess-2026-10-15.md#182-第五批统一写边界可恢复落盘与-g02026-10-18)。不恢复批量 LLM 重解析、真实迁移、部署或常驻实例操作。
- MP-0 第一批已验收：稳定记录身份/递增版本、精确目标/轮次守卫、重复上游消息号隔离、并发旧版本拦截、保存后原行/滚动保持；自动组暂拒绝半改。Rust 241、桌面 e2e 18、fmt/clippy/build/隐私通过。完整自动产物、动作句/代填、事务/撤回与独立投影仍待后续批次，不是新工作流整体完成。
- MP-1 第二批：新正常处理记录保存权限前完整动作、候选与原结果，重算不重解析；真实组指纹/历史、只读动作详情、原自动与当前结果/人工解释分层已接线。普通配置保存触发零模型政策重算；损坏或不匹配明确待核对，旧缺失不补造；锁期消息不吞并锁前认购。Rust 258/258、e2e 19/19、fmt/clippy/build 通过，最终隐私证据见专项 §16。完整选择模型/多动作 ID、动作编辑 UI、事务/撤回及所有写入口统一仍待后续，不部署。
- 第三批：单条完整 replace/ignore/reject/automatic、稳定动作/项与原 claim 身份、新保存接口及非 JSON 手工编辑器已接线；原自动只读对照、动作带入、排/撤/改、多项/多动作、解释、一次保存、冲突保留草稿与幂等均验证。Rust 271/271、桌面 e2e 20/20、fmt/clippy/build 通过；原文/后续操作保留、模型启用下保存/政策重算仍 0 调用。旧文件只读详情不迁移。仍待模型代填、组事务/撤回、独立投影与旧写入口收口；不是全部 41 项完成，见专项 §16–17。

### 桌面工作台（2026-10-13）

- 已实现：8 个页面统一方形工作台布局；1920×1080、100% 桌面验收；价格以元录入、整数分保存；阶段归入轮次管理；别名单行、切分按钮验证；新增只读当前激活轮次复盘接口，默认最新步，成员明细随步骤，切换轮次不混用旧数据。
- 验证：Rust 218 项、端到端 15 项通过；后者隔离轮次、消息、事件与结算目录，不连接真实群。
- 尚未部署：常驻进程未更新/重启。旧服务复盘只显示真实当前快照，并持续提示逐步历史尚未启用。更新前须备份当前状态并制定恢复方案，不能只重启后假设内存排位会自动恢复。远程展示仍暂停。
- 提交前遗留：隐私扫描仍有既有昵称命中，未通过前不得推送；本轮未将真实数据写入跟踪文件。
- 清理遗留：测试进程均已退出；本轮两处隔离临时目录删除被执行策略拦截，仍保留在系统临时目录。未停止常驻实例、未删除真实运行数据。

### 测试轮隔离与批量测试消息（2026-10-13，**已实现**）

> 口径见 [DECISIONS.md](./DECISIONS.md) §B-4；详细计划见 [SPEC-TEST-ROUND-ISOLATION.md](./SPEC-TEST-ROUND-ISOLATION.md)。
> 验收：`cargo test` 223 passed · `cargo clippy -- -D warnings` 通过 · e2e `16/16`（含 1920×1080/100% 排版）· `npm run privacy` OK。

| ID | 状态 | 任务 | 验收 |
|---|---|---|---|
| TR-01 | done ✅ | **轮次 `category`（test/normal）+ `gateway.test_group_id` + 绑定校验** | 单测：`category_issues` test/normal × 群匹配/不匹配 |
| TR-02 | done ✅ | **伪造解包隔离到 NC 入口**（`gate_nc`，仅测试轮+测试群）；`process` 去伪造 | 单测：`gate_nc` 开关×轮类×群组合 |
| TR-03 | done ✅ | **批量端点 `POST /api/sim/messages`**（裸数组/`messages`；逐条；缺字段/非测试群 drop；仅测试轮） | 单测 + e2e「批量注入」核对 summary/results |
| TR-04 | done ✅ | **单条 `/api/sim/message` 加"仅测试轮"门槛** | 非测试轮返回 400 |
| TR-05 | done ✅ | **移除模拟器页与伪造发送**（`sim.html`/`sim.js`/导航/`buildOneBotEvent`/`/sim` 路由/`_tools` 行）；保留消息编辑 | 全站无 `sim.html` 链接；e2e 断言 `/sim`、`/sim.js` 404 |
| TR-06 | done ✅ | **改写 `tests/e2e/sim.mjs`**：改走 HTTP 注入 + **1920×1080/100% 排版验收** | e2e 16/16；`devicePixelRatio===1`、无横向溢出 |
| TR-07 | done ✅ | **文档同步**（INTERFACES/FUNCTIONAL/DESIGN/GAP/web README/AGENTS/config.example/round-setup.template/e2e README） | 文档与实现一致 |

### 消息合并与匿名代排（2026-10-13，**已实现**）

> 口径见 [DECISIONS.md](./DECISIONS.md) §B-5；计划与校验修正见 [SPEC-MESSAGE-MERGE.md](./SPEC-MESSAGE-MERGE.md)。
> 验收：`cargo test` 230 passed · `cargo clippy -- -D warnings` 通过 · e2e 17/17。

| ID | 状态 | 任务 | 验收 |
|---|---|---|---|
| MR-01 | done ✅ | **配置 `merge.{enabled,same_sender}`** + `config.example.json` | 旧配置反序列化兼容 |
| MR-02 | done ✅ | **共享纯分组 `src/parser/coalesce.rs`**（同意图/相邻/边界/闲聊断开） | 单测 5 项 |
| MR-03 | done ✅ | **重放分组**（`session::compute` 代表处理+成员 `Merged`） | 组文本=join、状态正确 |
| MR-04 | done ✅ | **统一入口 `ingest` + `Pipeline::recompute`**（实时与 sim 共用；急切更新） | 追加==新消息一致 |
| MR-05 | done ✅ | **实时==重放 parity**（合并路径） | `realtime_and_replay_merge_match` 通过 |
| MR-06 | done ✅ | **匿名代排 `X(代)`**（身份 `发送者(代)`、`proxy_base` 按发送者校验、锁定仍生效） | `anonymous_proxy_uses_sender_permission` 通过 |
| MR-07 | done ✅ | **展示逐条 + `Merged` 徽标**（`display.js`/`replay.js`/`display.css`/`theme.css`；`state::messages_since` 不过滤） | 不合并展示；成员行可见 |
| MR-08 | done ✅ | **README 告警 + 文档同步** | 一致 |
| MR-09 | done ✅ | **e2e 适配**（`sim_routes` 走 `ingest`） | e2e 17/17 |

### 唯一事件处理链（2026-10-13）

> 口径见 [DECISIONS.md](./DECISIONS.md) §B-6；计划见 [SPEC-UNIFIED-PIPELINE.md](./SPEC-UNIFIED-PIPELINE.md)。

| ID | 状态 | 任务 | 验收 |
|---|---|---|---|
| UP-01 | todo | **阶段锁**（`RoundPhaseSet` 事件 + `can_claim(Settling/Locked)=false` + 移除 `State.locked`） | 单测：Settling/Locked 拒绝；锁后不复活 |
| UP-02 | todo | **computed 缓存**（`data/computed/<round>.json` + 完整性 + 失效 + activate 三模式） | 读写/哈希/失效单测 |
| UP-03 | todo | **唯一 ingest**（预处理全落盘→LLM override 落盘→增量 compute；删双路径） | 实时==重放；追加==新消息 |
| UP-04 | todo | **增量**（追加→尾部组；编辑/改时间→全量） | parity + 性能 |
| UP-05 | todo | **安全加固** F1/F2/F3/F7/F8 | 单测：伪造降权、round_id 校验、长度上限 |
| UP-06 | todo | **复盘按组**（computed 分步；组内追加不产生新步） | e2e/前端 |
| UP-07 | todo | **文档 + 测试** + e2e + 全量回归 | 全绿 |
| UP-08 | todo | **Git 历史清洗 → 重签 → force-push** | `%G?` 全 G、`git log -S` 0 命中 |

### P1

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-13 | done | **阶段权限接入实时链路**：实时+重放均按 `phase_at` 拒绝越权；A/B 分类由 `class` **自动推导**（有子项⇒A/无子项⇒B，`RoundSettings::item_class`）+ `/round` 编辑器设置 `kind` | A-5 | ✅ 越权实时==重放单测；分类自动推导单测 |
| T-14 | todo | **管理员改单目标语法**：`/改单 <目标> ...`（管理员改群内任意指定人）；需把目标昵称解析为 user_id（可用 `state.display/identity`） | D-2 | 管理员改他单用例 |
| T-15 | done | **`MessageLog::update` 接线**：`PUT /api/messages/:seq` 已由 `services::messages::update_message` 调用细粒度 `MessageLog::update` | — | ✅ 不再全量重写 |
| T-16 | doing | **私有快照入口已实现**：系统数据维护导出/预检/应用及备份已有；旧 `admin + format=file` 下载形态未做，不再增admin重复真源 | T-06 | 已有 correctness-gates / snapshot_tests；下载形态另列差距 |
| T-17 | doing | **现行复盘变化可视化已有**：三视图/标记/点击追溯；独立旧 `/api/replay/diff` 的去留随统一栈核实 | — | board-layout / replay-graph；旧API不冒充已迁移 |
| T-18 | done | **成员 user_id 映射**（NapCat 拉取写入缓存）：refresh 已写缓存（`data/members.json`），映射实现完成；待实机验证 | B-2 | 映射非空 |
| T-29 | done | **代排归属与真实发言溯源**：唯一当前群成员归乙/用乙资格，无“代”；匿名/群外/数字代号归甲的稳定代号/用甲资格；歧义/错误来源不猜，旧无actor先核对 | DECISIONS L | 2026-10-22：7项成员索引＋15项代理交叉/冻结/旧身份/逐prefix回归、旧哈希证明；32桌面含真实来源展开/同名不丢行，完整证据见工程计划§10 |
| T-28 | todo | **管理员阶段豁免**：`is_admin`（OneBot `sender.role` = owner/admin）在**阶段权限**中放行——`can_claim`/`can_cancel` 对管理员一律视为可排/可撤（不受 A/B 类别与 Phase0/II 限制）；**实时 == 重放**。伪造消息路径可用内层 `is_admin:true` 测试 | — | 管理员在 PhaseII 排 A 类不被拒；单测覆盖实时==重放 |

### P2

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-19 | done | **成员拉取健康与恢复**：`services/member_pull`按本地日历重排，手动/每日共用来源守卫、单飞与取消监控；成员页查看下次计划/最近成败，原按钮显式恢复，状态刷新不动草稿 | B-2 | 6项隔离证据、359 Rust与31桌面/只读语料通过；只用假接收器，未拉真实群，监控仅本次运行 |
| T-20 | done | **隐私守护及隔离自测**：card/CN/别名/自定义缓存/轮次，索引＋工作副本、vendor/自身密钥守卫、示例不能放行私有标识 | — | 2026-10-21：`npm run test:privacy`21项、249文件扫描通过；命中/源损坏失败且不打印秘密值，不等于历史清洗或完整DLP |
| T-21 | done | **FUNCTIONAL现行引用稳定函数/类型与源码链接**，旧混合快照归档，不再冒充当前限制 | — | `test:docs`校验本地链接与显式源码声明；E2开发入口同跑，历史引用只保留在archive |
| T-22 | done | **结算配置样例已有**：pricing/discounts/gift_tiers/reduce_average | — | 配置测试及30项桌面实际加载现行模板；不重复生成 |
| T-23 | done | **本地持久帧＋容量1唤醒＋单worker**；echo不等业务writer，绑定/收据恢复、故障暂停/显式重试和覆盖性操作守卫。不是有限磁盘或未落盘绝对无丢失承诺 | — | 256帧echo/FIFO、重启/组部分提交/坏历史/故障/轮次守卫；353 Rust、30桌面、只读语料及隐私通过，见 [合同](./SPEC-DURABLE-INTAKE.md) |
| T-24 | done | **simulate迁移当前确定性会话**：显式目录/理解/优先输入，旧ReplayEngine及专用壳删除；历史/当前fixture版本明确，报告兼容字段和元数据修正见工程计划§7 | T-11 | 334核心、30桌面、四组公开226条旧签名逐格基线及两套私有语料通过；12项直接对照含CN/冻结/逐prefix/组/撤改/override/权限/diff |
| T-25 | doing | **pipeline已目录化，verifier已拆fixture/execute/report/test**；后续新栈职责随T-24维持边界，不为行数拆分 | — | 原函数体不变；新场景与两套语料对照，不把拆分等同统一栈完成 |
| T-26 | doing | **核心与公开参考共382项**；继续按领域/重放/路由实际分支补证据，不按总数称全覆盖 | — | 明确性质/边界回归；入站/成员健康/代理身份直接证据不替代未审计分支 |
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
