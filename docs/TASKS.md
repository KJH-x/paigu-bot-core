# 排谷机器人 · 任务拆分（TASKS）

> 每个子 agent 必须先读 [POLICY.md](./POLICY.md)、[DESIGN.md](./DESIGN.md)、[AGENT-RULES.md](./AGENT-RULES.md)，再动工。
> 文件所有权见 DESIGN §2；**不得越界修改**。

## 当前执行：E4分批收口（2026-10-22）

2026-10-23 UP-02基础/恢复验收：主代理新增replay/computed及构建指纹，复用已有sha2作build-dependency（无新包），成员称呼由mtime改按内容失效；启动先严格源/缓存恢复及持久结算锁，再开接收。人工事务/配置/导入已提交结果更新派生物，写失败不回滚业务；原内存parked/fresh/显式replay区别保留。先真实红测无检查点，再以9管线/服务＋2内部边界证明跨实例、旧条/名单/政策/人工事务、损坏/超限/写失败、锁/轮次；最新440 Rust、34桌面、8入口/3文档/21隐私与297文件、全部公开/私有只读语料通过，见工程计划§16.1。普通入站持续检查点、尾组增量、有限T-26仍未完成，不把A批充当整个goal完成。

2026-10-23 T-25完成：先真实红测确认漏订单草稿、坏消息行仍成功；现统一完成记录构建/全部审计/草稿/封包与文件写入，保留本条控制前切面，不等待/确认自身NC帧。Pending源/旧文件拒绝导出或导入；HTTP idle守卫未放宽。6项控制专项、1项真实本地WS持久帧/上游重复/零出站、1项私有根选择，以及默认目录UUID/入口隔离断言通过；最新429 Rust、34桌面、8入口/3文档/21隐私自测与完整公开/私有只读语料全部通过。原关闭开关仍Applied仅记录、非管理员Rejected，检查实际零文件而非误把状态当执行。无新依赖、不改真实配置/群/模型/常驻实例；见工程计划§15.2，缓存有效余项和T-26继续。

2026-10-23 W-G2-07验收完成，T-25职责子项继续：主代理将rounds生命周期/IO与纯目录/类别诊断分开、原长内联测试目录化；原函数体与旧测试仅可见性/格式调整，对照无逻辑改写。新增同owner/跨owner各4×4来源矩阵、真实目录first-match、身份唯一性范围/必填、warn不自动改class/价格等5项性质，与原8项合13项专项全通过。最新421 Rust、34桌面、8入口/3文档/21隐私自测和完整公开/私有只读语料通过。保持角色复用合法，不增加噪声警告或新拒绝政策；新源码引用与旧接口路径同步，见工程计划§14。后续旧斜杠导出/派生缓存及签名推送仍未完成，不宣称整个goal完成。

2026-10-23 T-17完整验收完成：先真实复现GET旧结果陈旧和同round_id跨实例串用，后移除全局缓存/remember钩子，明确当前已保存动作与实时板只读比较；8项核心专项和现行桌面首步/列视图不受预览诊断影响的实际交互通过。T-25补具体检查入口的Cargo失败诊断可见性，以真实临时test链接失败验证且保持失败停机/私有输出保密。最新416 Rust、34桌面、8入口/3文档/21隐私自测、完整公开及两套私有只读语料通过。无新依赖，主代理统一接线，文档见工程计划§13；暂存的T-16及本批仍未签名推送，不冒充已发布。接续跨商品诊断/职责与有效派生缓存计划，不触碰待定业务或真实运行环境。

2026-10-23 T-16验收完成：主代理接线私有附件下载和原文件上传预检/应用，按职责拆出snapshot构建/恢复/传输及前端控制器，不加依赖、不重复admin页面。最新隔离407核心＋1参考、34桌面、两套只读语料/文档/隐私门禁通过，新增7项HTTP和1项桌面直接证据；实际明暗截图可读/无溢出，原大整数完整往返。另修复随机遍历导致用户多商品汇总顺序及混合认领误合并，保留完整结果比较并24个独立进程重复，见工程计划§12。旧斜杠导出有独立待审边界，不冒充全部导出渠道已统一；接续T-17/25/26等有效项。真实配置/模型/群及常驻实例不动，整个goal仍进行。

当前T-29/T-14前置由主代理统一修改settings成员索引、bus/log的可选发言来源、pipeline代排分段、session资格/合组和只读消息来源UI，接口新增见INTERFACES；不使用子代理。用户已确认群内乙用乙资格、群外代号用甲资格。先复现4项反查缺陷，补冻结旧指纹/历史接续和占位权限交叉回归；最新完整门禁与签名后方可结清本批，不能以代排代替T-14管理员目标命令。无新依赖，不改只读语料，不操作常驻实例。

T-29最终完整门禁已通过：382 Rust、32桌面、公开226条/两套私有只读语料、277文件隐私；7索引＋15代理＋1旧哈希直接证据及四张实际查看的明暗来源图见工程计划§10。已完成代排归属，不冒充T-14或全部E4完成；其他有效计划继续，真实部署/迁移仍未授权。

T-14已接入真实actor/执行开关、唯一目标、原文保留与共同Modify/权限，19项新证据含模型冲突、写失败名称原子性、后续撤销及手工修订/撤回；最新401 Rust、33桌面及全部只读语料/隐私门禁通过，明暗来源图已实际查看，见工程计划§11。主代理统一改bus、pipeline/names/target_modify、session、coalesce全角控制边界及既有来源窗口；无新依赖。待确认和暂停项未提权/恢复，下一项T-16下载形态，整体goal仍进行。

消息/桌面目标已在 `9bb8bf2` 签名推送完成，E0/E1在 `29d0200`、E2在 `871c7c8`、E3在 `57414a6` 签名推送；后继持续目标见 [工程计划](./PLAN-engineering-followup-2026-10-19.md)。当前主代理统一接线，不使用子代理。无接收器/真实配置的统一隔离检查、公开结算参考、可选私有只读语料及完整桌面门禁保持。E4本批T-23持久入站已通过353 Rust、30桌面与19项直接恢复证据，具体边界见 [合同](./SPEC-DURABLE-INTAKE.md)。剩余有效E4工程项和待决策仍未完成，不把本批签名当整个后继目标完成。真实配置/消息/模型及常驻实例不动，不部署。

E2已在 `871c7c8`（G）推送；E3首批将935行verifier按夹具/执行/报告拆分，新增3项容量/排序/重复/撤销/权限/报告字段基线。执行体未变、仍走旧ReplayEngine，不把职责拆分冒充迁移完成。下一批纯目录/理解输入及共享会话门禁见工程计划§6.1。

E4 T-23已在 `7baec0e`（G）签名推送。随后T-19成员每日计划、手动/自动成败监控、并发/取消与来源隔离接线完成，新增6项核心及1项桌面；最终359 Rust、31桌面、公开CLI/隐私/文档及两套私有只读语料通过，见工程计划§9。状态只代表本次运行，不是持久审计或后台存活认证；不部署、不拉真实群。其他有效E4项继续，不标整个goal完成。

E3第二批已实际替换执行器：显式目录/理解输入共用当前会话；旧ReplayEngine及专用领域壳删除，保留兼容报告DTO。默认历史夹具不新增改单理解，当前fixture_version=2使用当前规则；四组公开226条与旧签名基线实际逐格一致并固定回归，12项新证据覆盖CN、冻结、真实管线逐prefix、组/撤改/override、权限/范围与diff。最终334核心、30桌面、两套私有语料及隐私/文档门禁通过，T-24/W-G2-05收口；E4及业务待决策仍继续，不部署。

## 已完成：消息专项（2026-10-16—19）

本轮开始基线 `b4d0dca`（G）。第十二批后段补齐代填完整上下文/锁态/交付迟到守卫、中文输入与历史导航、旧条断线重连、订单生成整链、目录首中末与两套只读语料。MP-T01…41 和 IA 18 项已经逐条审计登记，见 [验收台账](./VERIFICATION-msg-ia-2026-10-18.md)；最终门禁已通过，签名推送并核对远程后更新 goal，再建立后继目标。核心319、完整桌面30、246文件隐私与两套只读语料通过，证据见台账。主代理统一接线，无子代理、无新增依赖；不部署、不操作生产数据。

用户补充授权（2026-10-19）：当前持续目标完成后，重新审计仍有效的已计划内容，再建立下一目标继续实施与测试。不得因该授权恢复文末已舍弃决定、明确暂缓的远程部署或真实群操作；在当前目标真正闭环前不伪造完成、不提前替换活动目标。

以下各批次段落是历史执行记录，不能将其中“仍待”重新当成当前未实现清单；当前专项以逐项台账为准，后继工程待办另行审计。

第三批单条核心已接线：主代理实现完整选择、稳定动作/项、保存接口与非 JSON 手工编辑器；沿用旧事件日志但区分版本化选择载荷，新增失败/幂等/版本/只读回归。Rust 271、桌面 e2e 20 已通过；实际终检见专项 §16，细化合同见 §17。无新增依赖、无子代理，合并事务/撤回/模型代填/全局投影版本不冒充完成。

本批 MP-1 核心接线已通过隔离回归：主代理统一接线 `messages/processing`、pipeline 理解产物、replay 复用、只读动作详情与普通配置保存后的政策重算；不新增依赖、不使用子代理、不操作生产数据。原自动结果与当前权限结果分开，旧缺失结果只作兼容读取，不猜测填造。完整动作选择/多行 ID、UI、事务/撤回、独立投影及热载等旁路统一仍未完成，实际门禁持续记录在专项 §16。

> 2026-10-16 当前消息专项由主代理统一跨模块接线，不使用下方历史并发流程。已授权按 [详细计划](./PLAN-message-reprocess-2026-10-15.md) MP-0…6 顺序实施，稳定记录身份/版本/轮次守卫首批已验收，再补自动结果、完整动作与事务/撤回。接口先登记 INTERFACES §8.9；不部署、不迁移真实数据。

## A1 · Gateway（`src/gateway/**`）

**目标**：反向 WS 服务器，接 NapCat 事件，白名单/drop，只读动作回包。

交付：
- `src/gateway/mod.rs`
- `src/gateway/onebot.rs`：`RouteMessageEvent`（post_type/message_type/self_id/user_id/group_id/message_id/raw_message/message/sender），`RoutePolicy{whitelist_groups}`，`RouteKind{Drop,Message}`，`decide_route`（非白名单/非群消息/空 → Drop），`normalize_message`（CQ 码/段 → 纯文本），`sanitize`（脱敏日志）。
- `src/gateway/ws_server.rs`：`tokio_tungstenite` accept loop，绑定 `AppConfig.gateway.bind`；每连接 task；15s ping；解析文本帧 → `decide_route` → Drop 记日志 / Message 推入 `mpsc<IncomingEvent>`；处理 `echo` 动作响应（oneshot 表）；`send_action(action, params) -> Result<Value>`（校验 `allowed_actions`，**禁止 send_***）。
- `src/gateway/action.rs`：`get_group_member_list(group_id)` 等只读封装。

验收：`cargo test` 通过；单测覆盖 `decide_route`（白名单/非白名单/非 message/空）；`echo` 往返可用 mock 验证。

## A2 · LLM + 配置（`src/llm/**`、`src/settings.rs`）

**目标**：DeepSeek 客户端 + 排谷流水线 + 热载配置。

交付：
- `src/settings.rs`：`AppConfig`（按 DESIGN §3）、`ConfigStore`（RwLock + revision）、`load/save/reload`、`notify` 监听热载、env 覆盖。
- `src/llm/mod.rs`、`src/llm/client.rs`：OpenAI 兼容 `chat/completions`（reqwest），`base_url/model/api_key_env/timeout/max_tokens/temperature`，JSON 模式，超时/重试。
- `src/llm/prompt.rs`：按 DESIGN §6 组装 system prompt（含商品目录/预存用户/时段/输出 schema）。
- `src/llm/pipeline.rs`：`process(event, cfg, engine) -> PipelineOutcome`：
  幂等 → 规则快速路径(`RuleParser`) → LLM 清理/抽取 → `EventValidator` → 权限(时段/预存) → 分配 → 回复文本（`reply_enabled` 决定是否发送）。
  LLM 失败 → `fallback_to_rules` 回退。

验收：单测用 mock LLM（不真连网）覆盖：排谷成功、非排谷 Ignore、歧义 NeedConfirm、时段拒绝、预存优先；配置 revision 冲突 409 逻辑。

## A3 · HTTP API（`src/api/**`）

**目标**：按 DESIGN §5 提供路由（端口 21081）。

交付：
- `src/api/config_routes.rs`、`board_routes.rs`、`display_routes.rs`、`sim_routes.rs`、`member_routes.rs`；更新 `src/api/mod.rs` 挂载 + 静态页服务（`web/`）+ CORS。
- `sim_routes`：复用 A2 流水线；`offset_ms` 仅改该消息 `timestamp_ms`。
- `display_routes`：`since` 增量（board + messages + who_whats + status + changed）。

验收：curl/单测覆盖各路由；`/api/display?since=` 增量正确。

## A4 · 前端（`web/**`）

**目标**：display / admin / sim 三页，vanilla，可部署。

交付：
- `web/display.html|js|css`：排位表 + 消息流 + who-whats + 状态；**5s 增量轮询**；keyed diff 只改变化格；**不打断划词/点击/滚动**（smart-scroll）；数据源 local/remote 适配。
- `web/admin.html|js`：config 编辑（prompt/商品/预存/时段/白名单/LLM/展示），`revision` 乐观并发 + 冲突提示；“拉取群成员”按钮 + 成员表。
- `web/sim.html|js`：身份切换/新建（**真实群成员**）、时间偏移输入（`±DD HH MM SS`）、消息发送、转录、实时排位。
- `web/index.html`：入口导航。

验收：`python -m http.server` 本地可打开；与 API 联调。

## A5 · 测试（`tests/**`）

**目标**：Node `.mjs` Playwright e2e + Rust 集成测试。

交付：
- 本仓库内 `npm i -D playwright`（独立安装，避免借用 browser-agent 环境）；`tests/e2e/sim.mjs`：启动本地服务 → 打开 `/sim` → 选身份/设偏移/发消息 → 断言排位与状态（含时段拒绝、预存优先、非排谷忽略）。
- `tests/e2e/README.md`：运行方式。
- Rust 集成测试补充（gateway 路由 / config 热载）。

验收：`node tests/e2e/sim.mjs` 退出码 0；输出 PASS 摘要。

## A0 · 主（装配与验收）

- `src/main.rs`：新增 `run`（默认，跑 Gateway+API+Pipeline）子命令；保留 `simulate`/`serve`。
- `src/app_state.rs` / `mod.rs`：接线 A1/A2/A3。（`app_state.rs` 已在 C-4 删除，现为 `src/main.rs` 直接装配）
- 合并验收：`cargo test`、既有 `simulation-corpus` 回归、e2e 全绿；更新 README。

## 依赖与顺序

- A1/A2 可并行；A3 依赖 A2 的 `ConfigStore`/pipeline 接口（先按 DESIGN §5 定契约，用 trait 解耦）。
- A4 只依赖 A3 的 HTTP 契约。
- A5 依赖 A3+A4 可运行。
- 冲突文件（`main.rs`/`mod.rs`/`Cargo.toml`）由 A0 统一改；子 agent 需要时在 `docs/TASKS.md` 追加“待 A0 处理”条目。

## 完成状态（2026-09-14）

| 项 | 状态 | 说明 |
|---|---|---|
| A0 接口冻结 | ✅ | `src/bus.rs`（IncomingEvent/EventSink/PipelineOutcome）、`src/settings.rs`（AppConfig + ConfigStore 热载）、`config.example.json` |
| A1 Gateway | ✅ | `src/gateway/**`：白名单/drop、反向 WS（`0.0.0.0:9801`）、心跳、echo 动作回包、拒绝 `send_*` |
| A2 LLM+Pipeline | ✅ | `src/llm/**`：DeepSeek 客户端 + 规则快路径 + 权限（时段/预存）+ 内存重放 + who-whats |
| A3 HTTP API | ✅ | `src/api/**`：config(409)/board/display/messages/sim/members/gateway-status + 静态页 |
| A4 前端 | ✅ | `web/**`：display(5s 增量、不打断)/admin(config 面板+成员拉取)/sim(身份+偏移) |
| A5 测试 | ✅ | `tests/e2e/sim.mjs`（Playwright，4 用例 ALL PASS）、`package.json` |
| A0 装配 | ✅ | `main.rs run`：Gateway+Pipeline+API(`:21081`)+每日 19:00 成员调度 |
| 引擎缺陷修复 | ✅ | `AllocationEngine` 快照 `boxes`/`user_summaries` 排序确定化（修掉 flaky） |

**验证**：`cargo test` 46 passed；`node tests/e2e/sim.mjs` 4/4 PASS；`real-samples` 逐格回归 ALL PASS。

**运行**：
```powershell
cargo run -- run            # Gateway(0.0.0.0:9801) + API(127.0.0.1:21081) + 每日19:00成员拉取
# 展示 http://127.0.0.1:21081/  ·  管理 http://127.0.0.1:21081/admin  ·  模拟 http://127.0.0.1:21081/sim
node tests/e2e/sim.mjs      # 端到端测试
```

**遗留/后续**：真实 NapCat 接入后 `reply_enabled=false`（不回复）——需用户确认后再开；成员拉取依赖 NapCat 已连接；Cloudflare 远程展示（R2 发布 + Pages）尚未接线。

## 整理阶段小结（2026-09-14，已结清）

- ✅ **脱敏**：`pipeline.rs`/`onebot.rs` 测试改占位名；`agent-d-adversarial/*`、`real-xlsx/README.md` 真实昵称 → 占位；`viewer/` 已删除（合并进 `web/replay`）。
- ✅ **真实名单策略**：真实群成员名单只放 gitignored `data/members.seed.json`；入库仅 `data/members.example.json`（占位）。
- ✅ **D2 安全 + 去重**：`reply_enabled` 强制（`send_*` 仅当开启且白名单才放行，默认 false 绝不发送）；删除 `require_token`；昵称/时段/重放/`describe_event`/`truncate` 收敛到单一真源。
- ✅ **D3 死码/旧栈**：删除确认死代码；旧栈加 `#![allow(dead_code)]` + `//! DEPRECATED` 隔离；`cargo build` warning **142 → 0**；测试 **52 → 63**。
- ✅ **D5 文档**：`README.md` 重写；新增 `docs/README.md`、`docs/MODULES.md`；`DESIGN`/`POLICY`/`FUNCTIONAL` 同步（删 `require_token`、`reply_enabled` 已强制、viewer→`web/replay`、`/replay` 路由、测试数）。
- ✅ **D4/A4 前端**：`web/admin.html|js` 移除 `require_token` 控件。
- **校验**：`git grep` 真实昵称在跟踪文件 **0 命中**。
