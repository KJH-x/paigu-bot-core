# 后继工程计划：先核实旧待办，再实施有效项

> 用户授权：前一目标完成后继续已计划内容，自行建立 goal。前一目标已在 `9bb8bf2` 签名并推送（G，本地/远程相等）：MP41＋IA18、319核心、30桌面、两套只读语料、隐私门禁完成。本文件是独立后继目标，不重新打开已完成的产品专项。
> 当前分支 `dev-paigu-full-closure`；仅隔离占位环境。不得部署/重启常驻实例、操作真实群/配置、调用真实模型、修改只读语料或重写共享Git历史。主代理统一接线，不使用子代理。

## 1. E0审计基线与执行资格（后续实施见§4起）

| 旧项 | 当前代码事实 | 后继处置 |
| --- | --- | --- |
| T-14 管理员目标改单 | `pipeline/admin::run_admin_command` 目前只支持开/锁/状态/导出，未提供目标用户改单 | 有效未完成；须保持身份与权限，不借消息override冒充命令已实现 |
| T-16 备份入口 | `snapshot_tests`、系统数据维护已有私有导出/预检/应用、完整日志备份；未提供原计划的浏览器下载形态 | 已完成安全路径入口；下载形态单列差距，不重复创建admin编辑真源，不公开备份 |
| T-17 diff可视化 | 当前轮复盘三视图已有逐步变化标记/追溯，`services/replay::replay_diff` 仍为独立旧API | 现行业务视图已覆盖；旧手动diff入口是否继续保留，随统一栈审计 |
| T-19 每日拉取与告警 | `main::spawn_members_scheduler`/`next_daily_ms` 已每日调度，失败仅tracing告警 | 调度已实现；可观测状态/失败恢复仍需补，禁止为测试触发真实拉取 |
| T-20 隐私守护 | 已有 `scripts/privacy-scan.mjs`，但成员源只取nickname，未覆盖card/CN/别名/自定义缓存与拆分轮次，缺隔离自测 | 优先补漏洞与可重复自测，不打印私有匹配值，不改真实私有数据 |
| T-21 文档稳定引用 | FUNCTIONAL仍大量引用旧 `pipeline.rs`/行号，路径已拆分 | 有效：改现行模块/函数名，校验链接/符号，保留注明历史的证据 |
| T-22 结算示例 | `config.example.json` 已有 pricing/discounts/gift_tiers/reduce_average，配置测试与e2e实际加载 | 已实现；补状态，不再添加另一份不一致样例 |
| T-23 有界入站/背压 | Gateway仍直接spawn到sink，业务writer串行不等于入站有界 | 有效；先受控压力/顺序/关闭测试，再设计有界通道，不静默丢消息或卡死echo |
| T-24 / W-G2-05 simulate与CN | CLI仍走 `simulation/verifier`→旧ReplayEngine；不共享已冻结理解/当前CN口径 | 有效；迁移必须保持输出合同及两套只读语料逐格一致，不给旧语料加新解析结果 |
| T-25 大文件 | pipeline已目录化，但verifier仍集中；新栈接线亦有较大模块 | 部分已完成；仅按职责拆分，避免无收益移动，测试≥120行独立模块 |
| T-26 补测试 | 已新增大量事务/引擎/服务测试，但不能据总数称领域/路由边界全覆盖 | 有效；按实际分支/性质补对照，不为数字堆测试 |
| T-27 远程发布 | 用户仍明确暂缓 | 不执行、不部署 |
| T-28 管理员阶段豁免 | 旧提议包含允许伪造内层is_admin；与测试降权/身份边界有冲突 | 待决策，不能因旧todo直接放宽权限，禁止绕过持久锁 |
| T-29 具名代理 | 已有切段/匿名代排测试；完整“名字→缓存user_id”及同名歧义仍须核对 | 有效审计；不把不确定名字自动归属他人 |
| W-G2-06 ColumnLocked | 语义仍无业务定稿 | 待确认，不擅自等同包尾或新增锁列政策 |
| W-G2-07 跨商品校验分级 | 原位动作已严格校验；目录同名/别名的提示级别须与现行匹配政策对照 | 先审计，再补明确分级与测试；不重解释冻结动作 |
| UP-01…07 | 共同writer、持久阶段/锁、按产物冻结组等已部分落地；磁盘computed/尾组增量仍没有 | 拆成现行有效能力与旧冲突要求，不照旧SPEC整篇执行 |
| UP-08 历史改写 | “全部未签名”已不符合当前G签名事实；force-push影响共享历史 | 暂不执行，需独立范围/备份/协作者协调确认；日常签名push继续 |

## 2. 阶段与门禁

1. **E0 审计/文档**：核实上表，TODOS分离已实现、仍有效、待决定和暂缓；旧SPEC冲突条款标记停止逐字执行。完成证据必须是函数/测试，不按旧todo重做。
2. **E1 隐私协作回归**：先在无远程的临时Git仓库复现card/CN/别名/缓存/轮次来源漏检，再补扫描；密钥/IP/禁止路径仍拦截，缺私有数据时公开协作者可运行，错误不输出秘密值。现有仓库扫描与全部新增自测通过。
3. **E2 稳定开发入口/文档**：提供可无NapCat/真实配置运行的隔离检查入口，私有两套语料可选但明确“未提供≠通过”；FUNCTIONAL改模块/函数引用，补源码链接验证。避免另建与现有30项UI旁路的模拟器。
4. **E3 simulate共用栈**：记录现有CLI输出格式/逐格基线，分离fixture读取/执行/报告；用当前确定性会话入口，显式适配历史夹具，不调用模型、不读真实缓存。CN仅来自传入夹具/显式测试配置。全量/分步、重复、撤改、组、权限、CN对照后才能切默认。
5. **E4 统一/背压与剩余**：在E3稳定后补有界入站及已确认剩余工程项，逐批压测/回归。computed是可丢弃派生缓存，不能取代不可变真源；缓存键必须包括修订/政策摘要，禁止只看last_seq而漏旧条修订。

示意（不是已实现API）：

```rust
// 当前确定性会话是真源；离线适配器负责显式输入，不构造Gateway或真实模型。
let records = fixture.into_message_records();
let result = session::replay_messages_with_parse(
    &config, &records, ReplayOverrides::default(), &overrides, manual_phase,
).await?;
// 输出兼容报告/逐格回放；不把模拟数据写入默认config/data目录。
```

## 3. 不得自动沿用的旧要求

- 不把merge默认改为开启；维持实验默认关。排与撤不合并，既有组按理解产物冻结。
- 不用seq/事件数作为旧消息修订游标，保持独立view_epoch/view_revision。
- 不恢复日常改原文、批量LLM重解析或覆盖旧文件式撤回。
- 不因缓存/阶段统一删除现有持久锁、绕过订单来源/版本守卫，或让结算锁期间可写草稿。
- 不擅自丢弃伪造测试的原timestamp或信任伪造管理员权限；必须对照已确认测试轮/批量排序合同再决定兼容策略。
- 不恢复远程发布/部署，不执行未经专门协调的历史改写。

每批：先冲突检查→失败回归→实现→针对性与完整门禁→隐私/文档→签名push。当前goal持续，未做的阶段保持未完成，不因计划文件已创建便宣称实施完毕。

## 4. E0/E1执行记录（2026-10-21）

- 接续工作树无外来冲突，基线仍9bb8bf2。旧todo已区分已实现/部分/待决策，旧UP草案停止逐字执行；T-22模板已有，T-16/17/19/25/26保持部分状态，不把未做形态打勾。
- 先失败后修复：六类新成员/配置来源漏检、vendor密钥豁免、暂存索引被干净/删除工作副本掩盖。扫描只读配置/轮次/缓存，覆盖原群名片/归一化称呼、CN/别名和数字身份；隐私源坏数据失败，不打印匹配内容/自定义私有路径。显式公开模板占位值与“空格”等非辨识通用词不误报。
- Git索引按NUL记录解析，批量按对象ID读取，索引及工作副本均扫描；vendor仅姓名词典豁免，密钥/IP连扫描器自身亦检查。无私有源时仍可运行通用检测。测试用本地临时Git仓库，无远程/成员接收器；清理先校验确切临时路径。
- 两处历史代码注释/测试中的真实称呼改为成员占位；不修改运行逻辑或只读语料。`npm run test:privacy`提供21类回归，另补示例文件被改写不能豁免私有标识的失败回归。只允许已知占位值，不能把任意示例内容当白名单。该检测不证明任意未知隐私已自动匿名化，不替代提交审查；E2…E4仍继续。
- 本批最终复验：319核心、fmt/严格clippy/构建、完整30桌面、21隐私自测、249文件隐私扫描与173个文档链接通过。没有UI/运行逻辑改动，仍完整重跑桌面门禁；隔离进程已退出，仅原常驻实例保留。T-20收口，后继goal保持进行中，E2…E4尚未完成。

## 5. E2执行记录（2026-10-21）

- 提供 `npm run check` / `check:full`，不需要NapCat/小号/私有配置/真实模型。所有运行时目录和系统temp均重定向任务临时目录；Cargo编译后直接从隔离cwd运行测试程序，避免Cargo的包根cwd意外读真实相对路径。首次真实试跑复现公开成员文件测试的cwd依赖，修正为明确仓库公开文件路径，不改业务代码。
- 可选私有样本只复制配置/队列/预期输入再重放，工作簿沿用只读助手并校验源哈希；遗漏明确NOT RUN，不折算通过，指定坏输入先失败且不显示路径。结算参考经核实已跟踪，因此默认复制公开参考并单独确保1项实际运行，不错误归类为必须私有输入或静默跳过。
- 完整桌面复用既有套件，清掉继承的专项过滤，避免只跑子集假全绿；中断只终止本次子进程树，清理前验证确切临时子目录。截图为本次临时产物，不能冒充新交付截图。无UI改动或新模拟器。
- FUNCTIONAL重写为现行合同与43个明确源码声明引用，旧混合快照完整归档（不是继续执行依据）；246个本地链接与声明校验接入统一门禁。T-21收口，非改行号掩盖旧“无持久化/复盘501/结算未接线”等已失效描述。
- 最终完整入口实际通过：318核心＋1公开结算参考、6入口自测、3文档检查、公开离线3消息、21隐私自测、257跟踪文件扫描、30桌面；显式两套样本逐格及2工作簿67消息/47组通过，源哈希不变。格式/严格clippy/构建通过。真实服务/群/模型/配置不动，仍未部署；E3/E4保持进行中而非整体完成。

### E2收口时的E3接续注意（历史基线，随后已执行§7）

旧verifier约935行，读取、命令特例、校验、ReplayEngine、报告混在一起；旧夹具可带已移除的商品box_size/特殊优先命令/自述购物金，新会话只从当前RoundSettings生成目录。不能简单把旧Item序列化回当前配置后就称等价：会丢容量、权限或盒列；也不能恢复已舍弃的serde旧键兼容。

下一批先按读取/执行/报告拆开，建立结构化输出和占位边界基线，再显式设计历史夹具适配输入、CN纯输入及共享确定性执行，逐格对照后切默认。旧ReplayEngine不能仅因改了名字就宣称已替换；模型/Gateway/默认成员缓存不得进入离线路径。

## 6. E3第一小批：分层与输出基线（2026-10-21）

- 接续基线为 `871c7c8`（G、已推送），确认无外来工作树冲突。`simulation/verifier.rs` 拆为 `verifier/{mod,fixture,execute,report,tests}.rs`；CLI参数、结果DTO与公开调用入口保持，读取/执行/报告既有函数体按去格式空白对照未变，仅报告函数的模块可见性调整。没有切换执行器或宣称T-24完成。
- 新增3项占位回归：显式旧目录的容量/价格/别名/优先输入；乱序队列排序、重复不重复执行、撤销推进与三个输出文件；半开优先时段及旧 `/加优先` 的具体归属/等级。保留原有逐格测试，测试超过120行独立模块。报告追加顶层、逐条、replay和步骤字段精确键集，防止未来适配悄悄删字段。
- 全套入口在分层后通过321核心＋1公开参考、30桌面、两套私有样本逐格与2源表67消息/47组，语料源哈希未变；追加字段断言后再复验322核心、6入口/3文档/21隐私自测、261跟踪文件扫描及两套私有语料亦通过。格式/严格clippy/构建、246链接/43源码声明通过。运行时业务逻辑及真实服务不改；后继goal仍进行，E3执行迁移、E4未完成。

### 6.1 E3第二小批实施合同（制定时尚未实现；落地证据见§7）

1. **纯夹具输入**：loader显式带入目录、优先/时段、阶段、CN/别名及测试身份，构造内存设置而非加载ConfigStore。成员映射只来自输入，不读默认缓存；禁用模型和出站。旧 `box_size` 等仅在fixture适配器处理，禁止重新加到当前设置模型或恢复已舍弃serde旧键。
2. **共用准备/执行**：当前 `session::prepare_understanding` 以cfg生成目录，需抽出接受明确目录上下文的内部纯准备函数；现有实时入口仍使用当前目录包装调用，离线入口显式传历史目录。旧验证器中已确认仅为夹具的授权/关团命令可转换为明确输入/控制产物，不给线上新增权限或相信伪造管理员。避免再写第三套自然语言解析/取消算法。
3. **冻结输入与身份**：适配为稳定MessageRecord及完整Understanding，先验证、再按原序列执行；重复上游消息号有独立记录身份但不重复动作。当前已冻结动作输入不得重跑模型或改别名重解释；旧纯文本fixture只走明示规则路径。CN覆盖用于消息及每步/最终摘要，代理不因名字映射越权。
4. **共用会话**：核心确定性会话接受显式只读目录及初始eligibility输入；默认实时路径仍由当前设置构造，不引入离线特例权限。政策/引用校验和分配走同一函数，商品旧容量、 scoped eligibility与各自valid_from不在转换时丢失。全部/逐步终态需相等，稳定ID和取消目标保留。
5. **兼容报告适配**：现有result/outcomes/report以及replay步骤字段保留，由共享会话事件/投影生成；步骤diff、结算、原消息标识与当前称呼不能缺失。旧ReplayEngine的版本初值/取消不加版本等实现缺陷不自动成为新业务规则，若发生可见值修正须单列说明并以事件/步骤一致性测试证明，不能静默漂移。
6. **切默认门禁**：公开占位覆盖CN优先、名字歧义、空位/盒容量、包尾、单领、撤改、重复/乱序、多动作/组与权限；对照当前会话和每个prefix终态。两个私有语料只读逐格、322起的核心、完整桌面、隐私/链接/源码符号全通过后才切默认。没有这些证据时保留明确旧执行，不能让部分可运行的新入口冒充迁移完成。

拟议边界示意（待实现）：

```rust
// 当前入口保持行为；只是把已存在的纯准备逻辑收在同一处。
prepare_understanding(cfg, record, parsed, source)
    // -> prepare_with_catalog(cfg, record, parsed, source, cfg.round.to_items())
// 离线适配器提供显式目录/身份/理解，不构造Gateway或真实LlmClient。
// -> shared_session(prepared_records, fixture_catalog, explicit_eligibility)
// -> compatible_report(shared_result)；不是旧ReplayEngine再跑一遍后改名。
```

## 7. E3共享会话迁移（2026-10-21，最终门禁已通过）

- `simulation::verify`准备完整MessageRecord/Understanding后进入当前 `session::replay_prepared`；实时包装仍取cfg目录，离线显式传目录/初始优先。准备逻辑抽出prepare_with_catalog，权限、取消、分配与配置重算共用compute。旧ReplayEngine与仅它使用的Round/RoundConfig/ItemAlias等壳删除；ReplayStep DTO仅放兼容报告模块，不保留另一套执行。
- 夹具不加载ConfigStore/Gateway/真实模型，不读成员cache。CN/别名、阶段、白名单、整盒关联与实验merge来自明确输入；历史box_size只留夹具适配，不恢复设置旧键。私有cache路径即使传入也忽略，模型/回复强制关；目录/优先同轮、记录身份完整唯一，未准备/坏历史不悄悄重解析。
- 旧公开C语料中Modify原为Unsupported，最初直接启用当前改单的逐格对照确实失败，未修改资产/预期放行。故显式fixture_version：缺省1保留历史理解限制，2使用当前规则；都执行当前核心。固定离线授权/关团/购物金语法仅为历史适配，成员/群白名单先于控制副作用，不能由被Drop的管理员增加优先权。未知版本失败，不影响线上管理员阶段待决策。
- 另复现两处真实漂移：同人后续昵称变化时实时取最新、旧重放取首次；StateDiff无序集合使相同报告不稳定。现统一最新有效称呼（重复/Drop不覆盖）、逐prefix称呼和稳定差异顺序；款式pair参与diff键，撤空消失格、同人换认购/策略不漏。补可选subitem_id，不改HTTP人工事务真源。
- 保留三个输出与原字段，补Dropped/Merged/Error计数。事件版本从0起且取消计入、input_message_count实际计数、replay.final_settlement实际最终值，均明确记录为旧元数据缺陷修正。每步同一生效行/分配器算prefix，完整CN进消息、摘要和账单，实际分配/等待不伪报成请求量。
- 签名旧基线0be05ad可执行文件与新执行器实际对照四组已跟踪公开语料：52＋49＋57＋68＝226条，全部格/空位/容量/策略/单领/等待一致。摘要来自旧版而不是新渲染器自证；固定进默认公开CLI回归，无需旧exe或私有源。不比较已声明修正的随机ID/时间/旧元数据。

新增12项隔离证据（migration_tests；不是靠总数推断）：

| 验收 | 直接证据 |
| --- | --- |
| CN/优先、每个prefix与完整报告确定性 | explicit_cn_priority_every_prefix_and_report_are_deterministic |
| 冻结动作不被别名改指、坏历史不回退 | frozen_actions_survive_alias_retarget_and_bad_history_does_not_reparse |
| 空队列/无商品、非法时间/私有cache守卫 | empty_bad_inputs_and_private_cache_are_never_silently_accepted |
| 实时管线逐prefix＝离线，merge开/关、撤/改单 | canonical_offline_matches_actual_live_pipeline_at_every_prefix_with_merge_and_modify；模型替身调用即失败 |
| 控制/重复/排撤组边界 | legacy_lock_duplicate_and_claim_cancel_are_group_boundaries |
| 阶段/群/成员权限与被Drop命令零副作用 | phase_foreign_group_and_whitelist_are_shared_without_admin_bypass |
| 不同容量、包尾/包盒、单领等待、款式diff与实际分配量 | different_capacities_tail_fullbox_single_and_step_diff_keep_variant_identity；容量在包尾改为强制列之前对照，不擅改U8 |
| 撤空列、同人新认购差异、每步称呼 | removed_columns_and_same_user_new_claim_are_reported_and_names_follow_each_prefix |
| 历史理解不新增改单，当前夹具支持 | legacy_fixture_modify_is_not_reinterpreted_but_current_fixture_supports_it |
| 多动作override保留原自动与后续撤销 | record_override_multi_action_uses_shared_recompute_and_keeps_later_cancel_and_original |
| 局部/晚授优先不能抹掉全局优先或追溯提权 | scoped_later_grant_preserves_global_priority_and_cannot_retroactively_prioritize |
| 冻结歧义在改别名后仍NeedConfirm | saved_ambiguous_understanding_stays_need_confirm_after_alias_changes |

成员白名单控制副作用守卫后的最终入口已全通过：333核心＋1公开结算参考=334，完整30桌面、公开CLI及四组226条、6入口/3文档/21隐私自测、263跟踪文件扫描、250本地链接/44源码声明；两套私有样本逐格以及2工作簿67消息/47组通过，源哈希未变。fmt/严格clippy/build通过。临时旧基线只用于实际对照，摘要固定后移除，可从签名Git基线重建；隔离服务均退出，仅原常驻实例保留，无部署。T-24/W-G2-05收口；E4及待决策仍未完成，goal不收缩也不标完成。

### E4接续发现（当时未实施；后续实现见§8）

Gateway::on_text目前在网络读任务内等待MessageLog writer，再spawn sink。writer跨模型等待时，网络读循环可能连后续echo都读不到；仅把spawn换成有界send仍不能证明回包可达。E4要先用阻塞sink/饱和入站＋同连接echo重现，再处理接入、持久化及控制读循环的边界，不能以“只有一个worker”冒充有界/无丢失。还须覆盖顺序、关闭/恢复与轮次/配置写边界；不连接真实接收器或因此放宽发送权限。

## 8. E4第一批：持久入站与恢复（2026-10-22）

实施基线为57414a6（G，本地/远程相同），无外来冲突。先以阻塞业务writer的本地WS实际复现echo超时，再接线私有原子文件＋容量1唤醒＋单worker；不是把饱和send移回网络任务。完整合同和限制见 [持久入站](./SPEC-DURABLE-INTAKE.md)。

- 帧接收绑定轮次/群/类别和独立UUID，子项part覆盖NC批次/代理。按磁盘序号处理，等待业务的帧不装内存队列。原始完整JSON与扩展字段/批次外层先同步留存；目标消息/事件日志同步后再确认，失败保留待处理文件。内存唤醒有界不代表磁盘/历史投影有界，未落盘输入、硬件断电及外部副作用无绝对零丢失/恰好一次认证。
- 故障暂停而不自动反复调用模型；失效worker可观察、显式恢复，不因新消息自动重启。启动会尝试恢复，旧轮积压挂起，激活后继续；共同writer内再次核对绑定。覆盖性操作加短接收栅栏和pending_ingress守卫；快照是完成消息切面，不含入站待处理原始帧。
- 保存后的处理收据重试不新增动作/行，不重复模型；真正上游重复仍有独立Duplicate行。Pending沿用稳定记录，组代表写完而成员未写时复用严格匹配的保存结果；坏历史不跳过后重解释。原自动指纹和人工事务真源不变。
- 全站暂停/积压警告直达设置接入，明暗、键盘、观察上下文提交及跨轮409已纳入桌面套件；正常禁用重试。按钮不批量重解析旧结果，未保存理解可能再次请求模型的边界明确。
- 验证中确实发现并修复：异步状态try_lock误判Pending、模型后写失败残留seen、Pending被误判Duplicate、组部分提交重问模型、加载收据误将测试reset改成replay、worker异常后无恢复入口、NC外层原始记录及确认前同步缺失。首轮桌面“录制→重放”失败保留并修复，没有削弱逐格断言；恢复状态比较仅去掉既有实时/确定性生成时间差异，认购身份/所有格/策略/总量仍完整比较。

测试模块新增19项：gateway/intake的256帧同连接echo/FIFO、取消恢复/轮次/暂停/panic、存储故障及原始检查点；pipeline/ingress的收据/子项/组/坏历史及破坏性守卫。最终完整入口通过352核心＋1公开参考＝353 Rust、30桌面、公开CLI四组226条、6入口/3文档/21隐私自测、269暂存/跟踪文件扫描、258链接/44声明、两套私有逐格和2工作簿67消息/47组（源哈希不变）；严格clippy/fmt/build通过。另单跑设置专项捕获1920×1080明暗图并人工查看，修正截图在主题过渡未结束时捕获假象的问题、补输入/恢复按钮≥4.5对比度检查，专项再次通过。截图留任务可视化目录，不入公开库。T-23收口；本后继goal仍含其他有效E4项，不能缩成此一批完成。

接续优先检查T-19每日成员拉取的本地可观测状态/恢复（仅替身测试，禁止真实拉取）；其后继续T-14/T-16/T-17/T-25/T-26/T-29和跨商品诊断/派生缓存的有效范围。ColumnLocked、管理员阶段豁免/伪造权限、远程发布及共享历史改写仍分别待决策/暂停，不为“全部做”擅自放宽。

## 9. E4第二批：T-19成员拉取可观测性（2026-10-22，隔离门禁通过）

基线7baec0e已签名推送、本地等于远程，工作树干净。已核实：现有每日任务按旧时间一次睡到明天，改时间不即时生效；失败只日志；成员页已有受轮次/配置/名单指纹保护的手动只读拉取，不另造无上下文重试入口。

1. 把调度计算/拉取监控独立成服务模块，单个进程内监控实例接入ApiState；每日与手动共用现有成员验证/缓存安装。只存最近尝试/成功、失败次数和当前运行，不记录名单/密钥/原始错误载荷。
2. 来源按轮次＋成员群＋缓存路径隔离；来源变化不把旧请求结果/旧成功展示成当前来源。并发拉取拒绝busy，取消有明确状态，不能永久卡运行中。既有迟到请求、指纹、名单数组/身份/缓存原子替换守卫保持。
3. 下一次本地时间可观察，按日历日期而不是固定24h；无效时间明确停用调度并告警，不默默改19点。配置时间/来源变化及时重排，执行前取得新上下文；一次失败等下个日程或人工拉取，不新增自动重试/真实群通知。
4. 成员页集中显示下一次、正在执行/最近成功/失败及恢复建议，复用“拉取群成员”按钮与草稿保护；状态刷新只读，不隐式覆盖称呼草稿或调用模型/拉取。
5. 先占位证明时间边界、无接收器失败可见/缓存不变、busy与取消、成功恢复和来源隔离；调度用可控时间/假接收器，不等待真实时刻或拉真实群。全量隔离门禁/文档/隐私后再签名push，T-19在完成前保持doing。

### 9.1 实施与证据

- 新 `services/member_pull` 保存有界进程内健康，ApiState单实例注入。原main固定24h/长睡函数移除；调度按日历下一次、严格HH:MM，约1秒观察时间/来源变更，启动或来源变更不立即补拉。无效时间暂停且可见，手动拉取仍可用；不增加自动重试、通知或模型调用。
- 手动与每日共用 `members::refresh_with_origin`：共同writer内完成当前上下文预检后才建立尝试，出站不持writer，响应后沿用原安装守卫。并发返回members_pull_busy、不发送第二次；Attempt析构记录中断并释放忙碌。拒绝把过期请求的失败挂到新来源；旧成功/迟到结果也不冒用新来源。
- GET members/workflow增加只读健康，成员页显示下一次、忙碌/最近成败和恢复提示；用已有轮询、按钮与草稿保护，不重绘名单，不因状态刷新拉群/调模型。未提供健康的服务显示未知，不伪报健康，也不因此破坏已存在的受保护手动入口。监控无名单/路径/原错误载荷，仅本次运行，不是持久审计或后台存活认证。

| 直接验收 | 证据 |
| --- | --- |
| 日历/月底/闰日/非法时间 | next_local_day_is_calendar_based_and_bad_times_are_not_repaired |
| 时间/来源重排、不因其他revision推迟、启动不补拉 | schedule_changes_replan_without_catchup_and_unrelated_revisions_do_not_postpone |
| 有界监控、隐私字段不公开、busy/取消释放、成功归零及来源隔离 | health_is_bounded_private_source_scoped_and_cancellation_releases_busy |
| 实际调度tick失败可见、缓存不变、不反复重试、非法时间停用 | scheduled_failure_is_observable_preserves_cache_and_does_not_loop_retry；可控时间＋无接收器 |
| 真本地WS echo成功恢复，busy不发第二次，成败与GET同源 | real_local_echo_recovers_failure_and_busy_does_not_send_a_second_request；仅假接收器只读动作 |
| 旧日程不请求新来源、中断不安装、再次恢复成功 | stale_schedule_and_cancelled_pull_cannot_install_a_new_source_cache |
| 桌面明暗、状态不动草稿、busy禁用、错误时间与显式恢复、实际离线502 | member-pull.mjs；1920×1080/100%，源码断言只发1次受保护手动请求 |

审查中纠正了旧请求预检失败误记为新来源失败的问题。UI测试最初碰到无user_id占位示例的只读保护，改用隔离目录内有标识的假缓存并恢复原文件，未强行启用只读行。全量曾在链接后高I/O负载下超过原8秒磁盘等待；同一测试程序单独256帧3.82秒全部到达，500ms echo断言通过。只将文件接受/排空等待改为30秒且增加实际状态诊断，**未放宽echo的500ms或FIFO/数量断言**。随后全量再次通过，不能靠重跑子集冒充全套。

最终最新源码完整入口：358核心＋1公开参考＝359 Rust、完整31桌面、6入口/3文档/21隐私自测、272暂存/跟踪文件、261链接/45声明、四组公开226条、两套私有逐格及2工作簿67消息/47组，源哈希不变。fmt/严格clippy/build通过；明暗截图已实际查看，单独成员健康专项亦通过。真实配置/群/模型及常驻实例不改，不部署。T-19收口；T-14/T-16/T-17/T-25/T-26/T-29、诊断/派生缓存的有效剩余和业务待决定仍在本goal内，不宣称总体完成。

## 10. E4目标身份查找（T-29完成，T-14前置，2026-10-22）

基线a03bb1d（G、已推送）工作树干净。核实旧反查只匹配清洗群名片，HashMap取第一个精确/包含结果，不识别CN/别名/明确ID，不核对缓存群来源。先用占位回归复现，再实现独立确定性成员索引：明确ID优先；精确群名片/CN/别名归一化后收集全部候选，只有唯一者可归属，部分匹配或同名必须提示、不猜第一个；错误/其他群缓存不跨群套用。每条分段消息使用同一索引快照，已保存动作不因改名/缓存变化重解释。

2026-10-22用户已明确：甲代乙排时，乙可唯一确认在群内（如MemberBeta）则按乙资格、归乙且表内不加“代”；匿名/群外/数字代号（代1、代X、代群外、代王哥等）按甲资格并显示甲(代标签)。数字代号不自动当QQ；明确@成员用于同名消歧。X/王哥等名字若未匹配群内成员，按群外处理；不把“群内未知”猜成某个成员。同名/部分匹配或缓存群来源错误须核对，不自动归第一人。

实现将保留真实发言人与归属的独立元数据；群外合成身份由真实发送者/群/代号稳定生成，显示名与内部身份分离，避免两个同名发送者的群外订单串在一起。新元数据进入新产物指纹，旧记录缺省不改旧指纹、不重解释；优先/阶段资格按用户新定口径。群内代排不等于允许普通成员撤改他人既有订单，仍遵守D-2，管理员目标改单另由T-14实现；代排分段不执行内嵌斜杠管理命令，不获得阶段豁免。

T-29同时补明确代排与已知成员行首“名字：”形式（与商品标题冲突时不猜）；所有分段归属先用同一成员索引快照，逐段持久化后实时/重放一致。管理员改单仍须真实管理身份、执行开关和持久锁；T-28阶段豁免仍待决定。全量/只读语料/文档/隐私后签名，未完成前不宣称T-29/T-14收口。

### 10.1 实施合同与迁移边界

```rust
// settings::targets：一次读取索引快照；每个标签的裁定跟随分段传递，执行时不重查。
enum MemberTarget { Known(String), Ambiguous(Vec<String>), Partial(Vec<String>), Unknown, Unavailable }
// 新日志可选delegation；真实actor不等于资格主体，更不能从被代理人的缓存角色推导管理员。
// Member => record.user_id为乙；Guest => sender_user_id为甲，record.user_id为稳定群外身份。
// ProcessingMember仅在Some时追加metadata，None的旧JSON哈希数组逐字不变。
```

- 已从旧HashMap首个/包含反查复现4项失败：同名、明确ID/CN/别名、部分名字、来源群不符。成员索引改为有序候选、NFKC精确、card-first，未在缓存中的CN不冒充群成员。缺缓存为空索引；损坏/重复ID/来源错误为Unavailable而非空列表。同一分段帧只裁定一次，不依赖全局mtime缓存。
- `pipeline/proxy.rs`按职责承接标记、已知行首名、归属和分段执行。数字代号与明确@区分；商品/成员同名和空段可观察，不以成功分段掩盖待核对。原外层正文/消息号、真实actor与目标类别进入私有日志和新产物指纹；修订窗口只读展开来源，不增加保存或模型请求。汇总按内部用户身份分组，避免同名甲相撞。
- 普通成员不可代撤改群内他人的既有订单，只有真实管理员角色沿用已有能力；本批不实现`/改单 <目标>`、不放宽阶段或锁。成员白名单仍约束实际发言人；资格按L的已知乙/群外甲两类。不同发言人的群内代理不自动合组；同一甲的匿名同意图合并仍保留。
- 旧已存理解不重问模型/换归属。旧匿名若明确sender且唯一，可沿用原合成身份接续/撤销，不改旧指纹；历史具名群外若没有真实actor或身份冲突，新增段标NeedConfirm，不静默建立另一身份导致旧订单遗留。没有进行真实历史迁移，这不是全部私有旧格式认证；损坏历史不跳过后猜身份。
- 初版新增回归曾因从未成功排位时看板为null而误假定对象失败，现显式验证零事件/零分配和重算后完整看板，不放宽已有认购对照。首轮379核心＋32桌面与两套只读语料通过后，再补旧匿名接续/旧具名缺actor两项，须最终最新全套重跑后才结清T-29；T-14等其他E4项仍继续。

### 10.2 直接验收清单（最终结果见§10.3）

| 范围 | 可重复证据 |
| --- | --- |
| 同名、部分名、ID/CN/别名/NFKC、card-first、错误群 | settings/target_tests七项；来源守卫也覆盖展示名缓存，防止跨群称呼借用 |
| 坏结构/重复ID、缺缓存、快照稳定/候选顺序 | malformed_duplicate_and_missing_sources_have_distinct_safe_results；candidate_order_and_snapshot_are_stable_while_cache_and_overrides_change |
| 乙/甲资格双向交叉、群内无代标签 | known_member_qualification_is_recipient_not_actor_and_board_has_no_proxy_label |
| 群外/匿名/数字代号、不能误认成数字ID | guests_numeric_codes_and_anonymous_use_actor_permission_not_another_members_id；循环各标签和正反资格 |
| 两个同名甲不串单、改名保持身份、汇总不并行 | same_name_proxy_actors_never_share_orders_and_rename_keeps_the_same_guest_identity；实际who_whats带稳定user_id，展示/复盘按它识别行 |
| 歧义/来源/未知@不调模型；商品同名/空段可见 | ambiguity_partial_source_mismatch_and_unknown_explicit_member_do_not_ask_model_or_guess；product_name_collision_and_empty_segment_are_visible_not_silent_partial_success |
| CN/别名/明确ID/行首名；不抢商品标题、不二次猜CN | cn_alias_explicit_id_and_known_line_prefix_resolve_without_stealing_product_headings；实际display API另覆盖其他成员card与既有CN冲突 |
| 保存后的乙不因改名/缓存换指 | saved_recipient_does_not_retarget_after_cache_and_alias_changes |
| 普通人不能代撤改群内他单、嵌套命令不锁团 | ordinary_proxy_cannot_cancel_or_modify_member_orders_or_execute_nested_control |
| 阶段按乙、无管理豁免、实际actor白名单 | phase_ii_checks_the_recipient_and_not_the_priority_proxy_actor；known_recipient_phase_and_manual_lock_apply_without_admin_exemption；actor_whitelist_cannot_be_borrowed_from_recipient_even_for_ambiguous_target |
| 隔很久撤回自己代号，不撤另一甲；匿名合并保留 | guest_cancel_after_gap_preserves_other_actor_orders_and_uses_the_same_frozen_owner；anonymous_same_actor_merge_stays_enabled_but_different_proxy_authors_never_merge |
| 旧匿名明确actor可接续/撤销；旧具名无actor不猜 | legacy_anonymous_order_keeps_its_owner_and_can_be_cancelled_without_migrating_old_hash；legacy_named_guest_without_real_author_needs_check_instead_of_silently_splitting_orders |
| 旧指纹不变，新actor不能替换 | old_input_hash_is_unchanged_and_new_delegation_cannot_be_swapped |
| 桌面真实API归属、三行中两同名不丢、来源展开/关闭、明暗/无溢出/不改数据 | proxy-attribution.mjs，1920×1080/100%；既有入站子段收据测试改为调用真正生产分段器，不保留仅测试用的旧算法 |

同时删除展示层按名字二次反查CN的旧链及其专用缓存包装，统一使用pipeline按ID计算结果；成员装饰/数字ID原测试继续验证真实生产入口，不保留无调用的测试专用函数。严查已捕获一处旧测试引用和一处因此变死的包装，修复后重新跑完整门禁，未压制warning或减少测试项。

### 10.3 最新最终门禁与接续

最新源码完整入口全部通过：381核心＋1公开结算参考＝382 Rust，32项1920×1080/100%桌面，四组公开226条、两套显式私有样本逐格及2工作簿67消息/47组（源哈希不变）；6入口/3文档/21隐私自测、277暂存/跟踪文件扫描通过。fmt/严格clippy/build通过。补文档后单跑链接/源码声明门禁，263链接/45声明通过。另单独运行真实来源桌面专项捕获群内/群外×明暗四图，已实际查看：原整条发言、资格依据、动作表单和保存/关闭完整可见，两同名汇总行同时保留，无页面横向溢出。截图留任务可视化目录，不入公开库。

T-29收口，T-25本批按职责拆出代理模块、删除旧二次CN反查；不把拆分和测试总数算作全覆盖。后继goal继续T-14管理员目标命令、T-16下载形态、T-17旧diff入口审计，以及T-25/26、跨商品诊断/派生缓存有效余项。ColumnLocked、管理员阶段豁免/伪造权限、远程发布和共享历史改写仍按待决策/暂停边界处理。真实配置/群/模型、历史数据迁移与原常驻实例均未操作，不部署；本批签名/推送不代表整个goal已完成。

## 11. E4管理员指定成员改单（T-14完成，2026-10-22）

基线4fee3b2（G，本地/远程相同），工作树干净；T-29已提供唯一当前群成员索引和实际发言来源，但`run_admin_command`仍不支持`/改单`。本批实现既有D-2，不恢复已舍弃的人工原文重写/批量LLM重解析，也不以普通代理改他人来冒充管理员命令。

1. 语法`/改单 <唯一群名片|CN|别名|成员ID|@成员ID> <更改后的商品与数量>`，目标单token，含空格称呼可用明确ID。中文全角输入归一化用于找目标；原始整条文本/时间/消息号不改，未知/同名/部分名/错误群来源及缺少参数明确核对，不产生群外代号订单。数字在此为明确成员ID，不套用代理数字代号语义。
2. **实际发言人**须为原消息管理员且执行开关开启；关闭只记录、不执行、不能因之后开启再重放生效。成员白名单先按实际发言人判断，不借目标资格进入系统。目标归属与实际actor独立记录为`delegation.target.kind=admin_modify`，模型不能选人/授角色。
3. 在分段/seen/上下文建立前完成目标规范化；保留同一message_id/ingress，不合组、不额外插一条伪造目标发言。规范化body只用于规则/模型理解，MessageRecord.text仍为原命令，实际actor和完整来源留私有日志。默认规则可理解则不调模型，复杂body走现有可替身的理解链。
4. 明确命令的语义为Modify：有效Claim/Modify抽取统一为修改（撤目标本人对应商品原认购＋新完整认购），模型不能把它变成独立加排；Cancel/Admin/Unknown等冲突结果只核对不执行。不支持0数量、嵌套斜杠/多目标控制语法，保持现有数量/商品/款式校验和first-match，不绕开原阶段/持久锁。
5. 资格按目标已知群成员，与T-29/D-2一致；管理员阶段豁免T-28仍未决定。保存权限前完整动作，当前政策拒绝可只逻辑重算；关命令执行开关不撤销已执行历史。目标/动作一旦保存不因名单/别名改名重新指派/问模型。
6. 实时、逐prefix、重算/重启、原自动/人工修订/单步撤回、后续成员自己撤改单、持久入站重试都验证同一原位置与目标。来源窗口显示“管理员改单来源”，可展开原命令和资格依据；32桌面基础上补真实点击/键盘、明暗/滚动/不改数据核对。全部占位/隔离，不调用真实模型/群/配置，不部署。

```rust
// 本批工程草案：权限和唯一目标先于规范化，record.text保持原命令。
// event.user_id = resolved_member_id;
// event.delegation = Some(Delegation { actor_*, source_*, target: AdminModify { body } });
// understand_input.text = format!("改 {body}"); // 仅理解视图，不写回原文
// understood.parsed.intent = Modify; // 仅在抽取为Claim/Modify且正常校验后
// evaluate_understanding(cfg, original_record, understood, phase) -> shared replay
```

### 11.1 已实施与直接回归（最终完整门禁见§11.2）

先以真实隔离Pipeline复现“未支持的管理员命令：改单”，未把计划当完成。新target_modify入口在原始日志后、seen/上下文前校验actor/开关和唯一目标，保留原text/seq/message_id/time/ingress；仅理解视图加“改”，新admin_modify来源进指纹。模型Claim强制Modify、冲突意图只核对；来源窗明确管理发言、不重复原话。执行开关、无目标、错群/白名单均在模型前结束，不调用真实模型/接收器。

首轮回归揭示两处名字漂移（即时拒绝或模型冲突时，旧实时snapshot姓名未随已保存来源更新；重放采用新姓名），不只是测试数量对不上。已把名字接线独立为names：有动作时持久化后才提交姓名，无动作但合法来源同步刷新姓名快照且不增加领域版本，Drop/重复不覆盖。另用实际写故障证明未保存命令不能发表新名/部分认购；原有按ID归属/冻结不变，不以删除姓名比较放行。

审查同时发现全角斜杠在merge开启时可能落入普通Modify。现入口/coalesce/历史控制兼容统一将其视为控制边界，错误`／改单@u2`明确拒绝，不按发言人自己订单执行或与下一条合组；新增目标索引仍唯一，不放宽权限。核心首轮399＋1公开参考、33桌面和两套只读语料已通过，后补整个商品/多款式/其他商品不被抹去的第19项后须再跑最新完整门禁。

| 直接范围 | target_modify_tests证据 |
| --- | --- |
| 原位置/目标替换，不改原文时间；管理员/其他成员订单不受影响 | admin_target_modify_replaces_only_the_selected_members_order_at_the_original_position；target_modify_preserves_actor_and_other_members_orders_and_real_command_source |
| 非管理员/开关关闭不执行，之后开关不补做 | non_admin_and_disabled_command_never_execute_even_after_switch_is_enabled |
| 歧义/部分/群外/来源错/坏缓存、缺参数/嵌套/错误全角前缀0模型 | explicit_target_lookup_and_argument_errors_do_not_call_model_or_create_guests |
| CN/别名/明确数字ID/@ID/NFKC与merge开启 | exact_id_cn_alias_and_fullwidth_commands_have_the_same_member_owner |
| 优先/PhaseII正反资格、actor白名单和活动群 | target_modify_priority_and_phase_use_recipient_and_do_not_borrow_admin_eligibility；target_modify_preflight_enforces_actual_actor_and_active_group_before_model |
| 模型误判Claim不变加排，冲突意图不执行；冻结/改名/关闭/重启0重问 | model_claim_is_forced_to_modify_and_saved_actions_survive_disable_rename_and_restart；conflicting_model_intent_never_turns_a_target_modify_into_cancel_or_admin_action |
| 手动锁/解锁历史、持久结算锁及坏锁文件 | locking_rejects_target_modify_without_discarding_understood_actions_or_later_unlocking_it；persisted_settlement_lock_and_bad_lock_state_cannot_be_bypassed_with_empty_runtime_state |
| 同一持久接收收据不重复，后续自撤原位执行/不合组 | target_modify_is_one_durable_receipt_and_retry_does_not_reapply_or_reask_model；later_self_cancel_keeps_its_position_after_target_modify_and_is_never_coalesced |
| 手工override/单步撤回保留后续消息和完整原来源 | target_command_manual_override_and_single_undo_keep_original_source_and_later_messages |
| 数量/商品校验不部分撤单 | target_modify_reuses_quantity_and_item_validation_without_partially_cancelling_old_claims |
| 真写故障不能提前发表新名；重复/Drop不能改旧名 | failed_target_modify_persistence_cannot_publish_new_names_or_partial_state；dropped_and_duplicate_names_never_replace_the_published_member_name |
| 元数据本身不授管理角色 | fabricated_target_metadata_does_not_grant_a_non_admin_role |
| 共享Modify替换目标的整个商品而保留其他款式购买者、其他商品 | target_modify_replaces_that_members_whole_product_without_erasing_other_products_or_users |

新增target-modify.mjs桌面真实API证明只记一条原命令/只改目标，键盘进出和来源展开、预填Modify/数量、明暗/滚动/无溢出、关闭返回原行且不保存；执行开关关闭后原三份不变、新五份不执行。测试只写隔离目录中的假缓存并恢复，常驻实例始终不变。

### 11.2 最终证据与后续

最后一次最新源码完整入口通过：400核心＋1公开参考＝401 Rust、完整33项桌面（1920×1080/100%），公开CLI三条＋四组226条逐格、两套显式私有样本、2工作簿67消息/47组（源哈希不变），6入口/3文档/21隐私自测和281暂存/跟踪文件通过；fmt/严格clippy/build通过。另单独再次运行管理员目标桌面专项，捕获并实际查看明暗两图：原命令只出现一次、实际管理员和资格依据、Modify/三份预填、保存关闭都可见，原行键盘返回通过，未保存/改数据。

T-14收口；当前分支仍不部署、不重启常驻实例、不调用真实群/配置/模型。文档将T-28/ColumnLocked明确改为待确认、T-27明确暂缓，未改变它们的业务口径，也不把它们当已实现。接续T-16私有备份浏览器下载形态、T-17旧diff API审计及其他有效E4项；T-25/26仍按职责/性质审计，不因测试数量或此一批签名宣称整个goal完成。
