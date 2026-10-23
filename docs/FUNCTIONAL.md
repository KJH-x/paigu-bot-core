# 排谷机器人 · 当前功能（FUNCTIONAL）

> 2026-10-21 源码复核。本文只列当前行为，引用模块/函数而非易失效行号；`npm run test:docs` 校验本地链接及本文显式源码符号是否存在。接口字段以 [INTERFACES](./INTERFACES.md) §8.9 为准，业务决策以 [POLICY](./POLICY.md) 与 [DECISIONS](./DECISIONS.md) §H 为准。
> MP41＋IA18 已逐项验收，证据见 [台账](./VERIFICATION-msg-ia-2026-10-18.md)。后继 E2…E4 不因此算完成，见 [工程计划](./PLAN-engineering-followup-2026-10-19.md)。未部署，不操作真实群/配置。

## 1. 运行与接收

`run`（默认）与 `serve` 装配同一 Gateway、Pipeline 和本地 HTTP 服务，不连接 PostgreSQL。启动先严格核验持久消息/人工审计，再使用有效派生缓存或确定性重算，并恢复持久结算锁后才启动接收；坏日志或不能恢复的结算锁明确失败，不猜测恢复成功。入口：[`run_gateway_stack`](../src/main.rs)、[`Pipeline::restore`](../src/llm/pipeline/mod.rs)、[`restore_persistent_lock`](../src/services/settlement/mod.rs)。

- Gateway 接收 OneBot 反向 WS，检查群消息、白名单、非空及成员限制，群名片优先。[`decide_route`](../src/gateway/onebot/mod.rs)、[`parse_identity`](../src/gateway/onebot/mod.rs)。
- 昵称清洗统一调用 [`clean_nickname`](../src/parser/normalize.rs)：全角归一，首个括号或空白截断；代理语义另保留身份，不能靠改显示名取得管理员资格。
- `send_*` 仅在 `reply_enabled=true` 且动作属于允许名单时才可发送；默认关闭。成员拉取是显式受限的只读动作，`reply` 字段不代表实际群回复。[`Gateway::send_action`](../src/gateway/ws_server/mod.rs)。
- Gateway 先本地持久接收，再以容量1合并唤醒驱动单 worker；等待业务不阻塞网络 echo，当前轮按接收提交序号处理。失败暂停保留文件，状态/重试在系统设置接入分区；磁盘不是有限队列，未落盘帧与断电不作绝对无丢失承诺。合同及隔离证据见 [持久入站](./SPEC-DURABLE-INTAKE.md)。[`Gateway::on_text`](../src/gateway/ws_server/mod.rs)。
- HTTP 仅监听 `127.0.0.1`，默认21081；没有公网鉴权。不可直接暴露管理 API。[`serve`](../src/api/mod.rs)。

## 2. 自动理解、权限与确定性排谷

[`Pipeline::ingest`](../src/llm/pipeline/mod.rs) 是入站入口。高置信规则优先，否则按启用/分段策略请求模型，失败按设置回退；不让模型决定排序或结算。[`Pipeline::understand`](../src/llm/pipeline/understanding.rs)。

自动理解产物完整保留权限判定前动作、候选、来源及当时结果。当前执行结果与原自动结果分开；重算复用已存理解，不因改别名而改指另一商品。坏产物/不匹配组明确待核对，旧无产物历史单独兼容，不能把新规则结果冒充首次理解。[`Pipeline::persist_understanding`](../src/llm/pipeline/understanding.rs)。

普通动作、忽略/拒绝/重复/Drop及控制命令成功保存后，都从同一组后缀计算发布当前消息/排表并更新完整检查点，不再各自拼State。控制命令先检查真实角色/群与成员白名单、完整日志和持久锁，再执行一次live副作用并保存terminal自动结果；之后重算/收据重试不执行命令。消息已提交而发布失败会保留理解、标记待恢复，不把它当未提交重问模型；缓存写失败不让已提交业务失败。[`Pipeline::publish_completed`](../src/llm/pipeline/live.rs)。

权限按照消息时间和当前配置判断，包括白名单、优先名单/半开时段、阶段和持久锁。优先名单改变只更新权限并确定性重算，不重跑模型。统一重放入口是 [`replay_messages_with_parse`](../src/replay/session/mod.rs)，阶段政策为 [`phase_rejection`](../src/parser/policy.rs)。

排位区分拼团、单领、整盒与特典；原价/子项调价以整数分存储，前端统一元。包盒策略与整盒商品不是一回事；包尾按已确认列规则分配并参与强制成盒。分配真源是 [`AllocationEngine`](../src/engine/allocation_engine/mod.rs)，生效行/撤销真源是 [`collect_effective_claims`](../src/engine/replay.rs)。

排与撤不自动合并。原申购后没有再次加排同商品时，后来撤销仍针对原申购，不要求相邻。前序修订让精确引用失效时，保留原引用并说明“撤销未命中”，不改撤另一认购；仍继续执行之后的撤排/改单。

实验合并 `merge.enabled` 默认关闭。开启时只合并同人相邻同语义理解单元，不推迟首条处理；实际分组及原产物保留，不能在改配置时重切冻结组。[`message_groups`](../src/replay/session/mod.rs)。具名代排唯一匹配当前群名片/CN/别名则归被代排人、按其资格；匿名/群外/数字代号按原发言人资格、显示发言人(代标签)。同名/部分名/错误群来源不猜；`@成员ID`明确指定群内人。旧缺少发言人身份的代排不静默转成另一订单，提示核对；保存后的理解/归属不因缓存改名重指。消息修订可展开原始整条发言和资格说明；保存/重算不重新问模型。

管理员可输入`/改单 成员乙 徽章 甲 3`，或用`@成员ID`明确目标：撤掉目标本人对应商品的旧认购，再记更改后的完整认购；保留其他商品/成员订单，不是多加三份。需要原发言人的真实管理员角色、执行开关、群/成员白名单；目标身份不唯一/无缓存来源、缺参数或冲突意图不执行，不创建群外代号。权限仍按目标成员和原阶段/锁判断，不增加管理员豁免。原命令保留且只生成原位一条记录，修订窗口显示管理员来源和修改动作；开关关闭只记录未来命令，之后开启不补执行，关闭也不撤掉已保存历史。日常纠错仍通过动作编辑器，不要求发此命令或重新问模型。

## 3. 管理员原位修订（非 JSON）

排谷消息流的每个状态均可打开统一动作编辑器。默认复制当前动作，点改排/撤/改单、多项、数量、策略并附管理员解释。原消息文本、身份、时间和自动产物只读；已处理记录不允许靠旧原文维护接口绕开修订。

- 选择完整动作替换、忽略、人工拒绝或恢复自动；空动作与忽略/拒绝不混为一谈。[`read`](../src/services/message_actions/mod.rs)、[`save`](../src/services/message_actions/mod.rs)。
- 一次保存立即在原消息位置重算，不强制模型或二次确认；之后的新消息/撤排不被旧快照覆盖。
- 连续或不连续补充由人工选取，等效于“最早主条完整动作替换＋补充条人工拒绝”。同一事务原子提交；自动组不可半改，解除补充可恢复其精确前选项。
- 最近一次人工事务支持单步撤回，使用逆事务恢复前次选择，不用备份覆盖整个当前状态。政策/锁态/目标版本变化会阻止不安全撤回，失败不消耗撤回槽。[`undo`](../src/services/message_actions/transaction.rs)。
- 模型代填仅在显式点击后请求，同一草稿可手改/取消。响应抵达时重查轮次、配置、目标/补充版本与锁态，迟到结果不覆盖编辑或自动执行。保存和撤回零模型。[`suggest`](../src/services/message_actions/suggest.rs)。
- 保存带轮次、配置/记录版本和幂等事务标识；并发修改/半截审计日志/写失败不报告成功。请求失败保留输入。独立 `view_epoch/view_revision` 通知旧行改变，不以事件数或最大seq代替。
- 需要改优先名单/阶段时可携草稿去设置再返回。草稿只在本标签会话保存，URL不放消息/名单；返回重验上下文，不自动加成员、切轮或执行。

详细 UI、请求与不可变产物合同见 [消息计划](./PLAN-message-reprocess-2026-10-15.md) 和 [INTERFACES §8.9](./INTERFACES.md#89-消息动作人工修订2026-10-16分批实施)。

## 4. 工作区、轮次与配置

七个工作区固定同序互达：总览、轮次与商品、排谷、结算与下单、复盘、成员、系统设置。旧admin/消息/成员入口只作兼容跳转，不能成为另一个编辑真源；模拟器页已移除。路由装配：[`build_router`](../src/api/mod.rs)，桌面说明：[web/README](../web/README.md)。

轮次库以 `active_round_id` 指向私有 `data/rounds/<id>.json`。创建/阶段/当前设置归轮次管理；商品四类目录独立编辑。切换与重放解耦；界面不提供“激活并清空”，已有后台fresh合同不等于日常推荐操作。[`activate_round`](../src/services/rounds/mod.rs)、[`setup_round`](../src/services/rounds/mod.rs)。

目录结构诊断与匹配分开：跨商品款式名/款式别名复用合法，仍按既定目录first-match；同商品自己的名称/别名不自撞。真正跨商品涉及商品名/商品别名的冲突、重复身份和种类结构不符为error；显式class与推导不符和缺价为warn，不自动改目录/价格/class，不重解释已存动作。[`check_issues`](../src/services/rounds/check.rs)。

系统设置分接入、消息理解、展示默认、数据维护，一次显示一组。业务区显式独立保存，在最新配置上合并该区允许字段；同区冲突保留输入，不顺带提交其他区草稿。轮次/成员/结算/消息亦有统一离开草稿保护；此时才询问保存/放弃/继续，不给每次消息保存增加确认。

所有会影响当前投影的配置保存、磁盘重载、监听热载、锁/轮次和私有导入统一经过 writer，先准备结果再可恢复落盘，成功后发布。[`apply_locked`](../src/services/configuration.rs)、[`ConfigStore::spawn_watch`](../src/settings/mod.rs)、[`persist`](../src/settings/commit.rs)。`gateway.bind` 改动仍不在线重绑，心跳按连接读取；不误称所有配置都无条件热生效。

## 5. 成员与隐私边界

名单来源按配置缓存→私有seed→公开example→空回退；群名片优先，数字身份归一。CN绑定user_id，可编辑；显示回退CN→清洗称呼→user_id，人工编辑后停止自动填充。[`get_members`](../src/services/members.rs)、[`member_name`](../src/services/members.rs)、[`resolve_cn`](../src/settings/mod.rs)。

显式拉取/称呼建议需当前轮次、配置版本与名单指纹。请求完成后再次检查，坏名单、未知/重复建议、缓存原子写失败均保留旧数据。模型关闭不发送建议请求；建议只预览选入草稿，再显式保存。[`refresh_members`](../src/services/members.rs)、[`suggest_cn`](../src/services/members.rs)。

每日拉取按本地日历时间安排，时间/来源变化后及时重排；无效HH:MM明确暂停自动任务，不默默改时间。成员页只读显示下次计划、运行中和本次运行的最近成败/连续失败；复用受上下文与草稿保护的“拉取群成员”重试，不自动重试或向群告警。并发拒绝叠加，取消释放忙碌状态；来源变化不冒用旧成功。此状态不含名单/原始错误载荷，重启不保留，不能把无接收器称作成功刷新。[`spawn`](../src/services/member_pull.rs)、[`Monitor::snapshot`](../src/services/member_pull.rs)。

公开协作者不需要真实名单、模型或NapCat：`npm run check` 运行公开占位核心/脚本/离线CLI/文档/隐私门禁，`npm run check:full` 额外跑既有完整桌面套件。私有夹具需显式提供，遗漏显示NOT RUN，详见 [开发检查](./DEVELOPMENT.md)。

私有配置/消息/成员/备份/工作簿不入库。隐私扫描同时检查索引与工作副本，覆盖已知群名片/CN/别名/私有配置及密钥/IP/禁止路径，但不保证未知数据已匿名化，也不自动清洗历史。[`privacy-scan.mjs`](../scripts/privacy-scan.mjs)。

## 6. 排表、消息流与复盘

排表姓名中央仅姓名，共用“三字宽/全长”偏好。三字按实际字体测量，空位同宽；全长按商品内同盒列最大名宽，格间2px/盒间4px，不均分撑满。实际容量/空位保留，超宽在商品内容内横滚。上沿2px盒色、下沿4px形状状态，全文/盒号/变化附提示与无障碍标签。字体加载后校准，复盘按全部已载入步骤算宽，切步不跳列。共用渲染入口：[`createBoardRenderer`](../web/common.js)。

消息及排位增量按稳定身份更新，不重建整页，不打断划词/点击/滚动；只有贴底才自动跟随。旧条修订通过投影游标补更，保留筛选定位行，清除定位后才移出筛选。[`display_with_cursor`](../src/services/display.rs)、[`syncKeyedChildren`](../web/common.js)。

复盘默认读取当前激活轮次实际消息/动作，最新步、成员汇总、表格/列/图，追溯可去同一消息处理。关系图默认当前商品，支持分组全部、单领/等待、姓名/款式定位、键盘与明暗；绘图本地无CDN。视图切换保留各自视区/步骤；当前轮旧条改解释也刷新。手动导入明确只读，不改当前排位，不回退假示例。[`active_view`](../src/services/replay.rs)、[`Pipeline::replay_view`](../src/llm/pipeline/replay_view.rs)。

旧差异入口另明确为当前激活轮已保存动作与实时投影的一致性诊断，共同writer/配置读锁内重新计算，不借其他实例或上次预览；未初始化实时板明确不可比较，不自动恢复/应用。显式覆盖预览仍在本次POST响应中，不污染后续诊断；时间戳/昵称样式不作为业务差异，比较范围为既有位次/认领/汇总/结算及领域版本。[`replay_diff`](../src/services/replay.rs)、[`do_replay`](../src/services/replay.rs)。本批验收见工程计划§13。

## 7. 结算、下单与备份

结算真源是 [`evaluate`](../src/settlement/engine.rs)。目录仅子项调价，调价不参与折扣/减均/特典档位；规则只试算与显式独立保存，不顺带保存订单。

排位/权限/目录/称呼/规则变化，旧订单保留但提示需重新核对、旧来源试算。普通后台保存不能清掉标记；管理员显式保留当前订单复核才记录新来源基线。锁定前检查数量、目标/价格和唯一订单号，问题可定位实际输入。重排包单独显式触发；正式页面不注入演示订单。[`put_draft`](../src/services/settlement/mod.rs)、[`lock_draft`](../src/services/settlement/mod.rs)，来源守卫见 [结算模块](../src/services/settlement/)。

私有导出/预检/显式应用支持完整日志和人工选择，保留当前配置/订单草稿；恢复可重试，不重注入NC或请求模型。系统数据维护另提供明确隐私确认后的浏览器附件下载及选择文件预检；文件/路径互斥、明确应用，原字节不经浏览器JSON重打包，64MiB以上改用主机路径，不截断。下载只含已保存数据、不含页面草稿或待处理帧，不是成员缓存灾备；仅本机可信环境，不能上传公开Issue/PR。[`snapshot_export`](../src/services/snapshot/mod.rs)、[`snapshot_import`](../src/services/snapshot/restore.rs)、[文件传输](../src/services/snapshot/transfer.rs)。本批验收状态见工程计划§12。

管理员“导出”复用同一严格读取/完整审计与草稿/封包/文件写入，但保留本控制记录保存前的已完成动作切面，不等待或确认自身NC收据。原始审计可含未执行帧，不恢复待处理队列，不是接收器灾备；HTTP导出与破坏性应用仍保留idle守卫。损坏行或Pending消息拒绝导出/应用，不跳坏行写空备份。默认目录随私有消息存储根（或明确PAIGU_SNAPSHOT_DIR），每次唯一目录避免覆盖；显式路径合同保留。[`Pipeline::export_snapshot`](../src/llm/pipeline/export.rs)。

## 8. 测试与仍未完成项

可重复检查见 [DEVELOPMENT](./DEVELOPMENT.md) 与 [桌面套件](../tests/e2e/README.md)。桌面仅验收1920×1080、100%、DPR1；计数以实际输出/台账为准，不按总数推断业务全覆盖。私有样本与源表只读，输出在隔离目录；缺私有输入不声称语料通过。真实接收器/真实模型/公网发布不属于自动测试。

`simulate` 已经从旧ReplayEngine切到当前确定性会话：[`run_cli`](../src/simulation/verifier/mod.rs)、[`replay_prepared`](../src/replay/session/mod.rs)。只读夹具显式提供目录/容量、阶段/优先、CN/别名，不加载真实配置、缓存或模型；当前夹具用 `fixture_version:2`，缺省1保留历史夹具的未支持改单理解，不给旧语料新增动作。两种版本都用同一准备/权限/取消/分配逻辑，旧执行器及仅它使用的领域壳已删除，未恢复旧serde键。

消息、每步/最终排位与结算显示CN；无CN按对应步骤最新有效称呼回退，重复/Drop不污染当前名。报告版本按实际事件数从0起，取消也增长；input_message_count与最终结算不再返回旧默认空值。步骤diff按商品/款式/盒/格比较，包含撤空消失格、同人换认购/状态变化，顺序固定；款式差异附可选subitem_id。原result/outcomes/report与步骤字段保留，新增Dropped/Merged/Error计数。格式与验证见 [DEVELOPMENT](./DEVELOPMENT.md)。

管理员目标改单、具名代理、有界入站、调度可观测性及跨商品分级已分批验收，证据见 [TODOS](./TODOS.md)/工程计划。私有派生缓存基础与恢复已接线，完整来源/配置/成员内容/当前阶段/源码依赖指纹及payload哈希失效，坏缓存回退、坏真源失败。人工事务、配置业务重算和私有导入提交后更新检查点；只读预览/诊断不因此写缓存。[`checkpoint`](../src/replay/computed/mod.rs)。

共享会话按完整冻结组/人工选择签名复用相同前缀，从首个变化组起求值全部后缀；仍全量分配并刷新完整输入的称呼，不拿旧名/资格或只看seq。普通与合并追加、控制/代理/terminal和已完成收据恢复均接相同提交后推进；备忘仅在本轮内存（随parked保留），重启首条可完整热身。旧无自动产物Duplicate仍是终止记录，不回退规则再认购；重复/Drop不更新已初始化的业务快照时间或当前称呼。最终有限覆盖审计仍待T-26，不按数量推断全覆盖。[`replay_incremental`](../src/replay/session/incremental.rs)。

派生文件默认在私有消息根旁computed，最多读取64MiB，可删除重建，不是备份或待处理队列。continue保留本轮进程内状态；冷轮从有效缓存/完整真源恢复，fresh清空当前投影并移除该轮缓存。非空历史建立进程内原记录身份cut，合并/收据重试不跨cut恢复旧认购；新消息即使时间更早也纳入。测试HTTP reset同样处理，部分板不写成完整源检查点。cut不删源/不落盘，显式全量replay/人工事务仍按完整真源并结束cut，重启仍完整恢复。复盘逐原消息可达、步骤不折叠，模式/游标不落缓存。ColumnLocked与管理员阶段豁免仍需业务决策；远程发布暂停，未部署或重启常驻实例。

## 9. 已放弃／历史内容（不可据此重新执行）

2026-09-14调查与之后分批记录完整移至 [历史快照](./archive/FUNCTIONAL-2026-09-14.md)。其旧行号、52/8测试、已删除的pipeline.rs/ws_server.rs等路径，以及“复盘501”“结算未接线”“无持久化/需主动恢复”“serve走旧栈”“模拟器仍可达”等描述不再有效。

不恢复：日常改原始消息、批量LLM重解析、覆盖整文件式撤回、按事件数/seq判旧行更新、默认开启merge、删除持久锁或接受伪造管理员提权。修正动作完整替换，原自动理解与后续操作保留；这些与旧提案冲突时以DECISIONS §H和现行消息计划为准。共享历史强推与公网发布需独立授权，不在当前自动执行范围。
