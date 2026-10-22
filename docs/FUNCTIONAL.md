# 排谷机器人 · 当前功能（FUNCTIONAL）

> 2026-10-21 源码复核。本文只列当前行为，引用模块/函数而非易失效行号；`npm run test:docs` 校验本地链接及本文显式源码符号是否存在。接口字段以 [INTERFACES](./INTERFACES.md) §8.9 为准，业务决策以 [POLICY](./POLICY.md) 与 [DECISIONS](./DECISIONS.md) §H 为准。
> MP41＋IA18 已逐项验收，证据见 [台账](./VERIFICATION-msg-ia-2026-10-18.md)。后继 E2…E4 不因此算完成，见 [工程计划](./PLAN-engineering-followup-2026-10-19.md)。未部署，不操作真实群/配置。

## 1. 运行与接收

`run`（默认）与 `serve` 装配同一 Gateway、Pipeline 和本地 HTTP 服务，不连接 PostgreSQL。启动先从持久消息/人工审计恢复，再接受入站；坏日志或不能恢复的结算锁明确失败，不猜测恢复成功。入口：[`run_gateway_stack`](../src/main.rs)、[`restore_persistent_lock`](../src/services/settlement/mod.rs)。

- Gateway 接收 OneBot 反向 WS，检查群消息、白名单、非空及成员限制，群名片优先。[`decide_route`](../src/gateway/onebot/mod.rs)、[`parse_identity`](../src/gateway/onebot/mod.rs)。
- 昵称清洗统一调用 [`clean_nickname`](../src/parser/normalize.rs)：全角归一，首个括号或空白截断；代理语义另保留身份，不能靠改显示名取得管理员资格。
- `send_*` 仅在 `reply_enabled=true` 且动作属于允许名单时才可发送；默认关闭。成员拉取是显式受限的只读动作，`reply` 字段不代表实际群回复。[`Gateway::send_action`](../src/gateway/ws_server/mod.rs)。
- Gateway 先本地持久接收，再以容量1合并唤醒驱动单 worker；等待业务不阻塞网络 echo，当前轮按接收提交序号处理。失败暂停保留文件，状态/重试在系统设置接入分区；磁盘不是有限队列，未落盘帧与断电不作绝对无丢失承诺。合同及隔离证据见 [持久入站](./SPEC-DURABLE-INTAKE.md)。[`Gateway::on_text`](../src/gateway/ws_server/mod.rs)。
- HTTP 仅监听 `127.0.0.1`，默认21081；没有公网鉴权。不可直接暴露管理 API。[`serve`](../src/api/mod.rs)。

## 2. 自动理解、权限与确定性排谷

[`Pipeline::ingest`](../src/llm/pipeline/mod.rs) 是入站入口。高置信规则优先，否则按启用/分段策略请求模型，失败按设置回退；不让模型决定排序或结算。[`Pipeline::understand`](../src/llm/pipeline/understanding.rs)。

自动理解产物完整保留权限判定前动作、候选、来源及当时结果。当前执行结果与原自动结果分开；重算复用已存理解，不因改别名而改指另一商品。坏产物/不匹配组明确待核对，旧无产物历史单独兼容，不能把新规则结果冒充首次理解。[`Pipeline::persist_understanding`](../src/llm/pipeline/understanding.rs)。

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

轮次库以 `active_round_id` 指向私有 `data/rounds/<id>.json`。创建/阶段/当前设置归轮次管理；商品四类目录独立编辑。切换与重放解耦；界面不提供“激活并清空”，已有后台fresh合同不等于日常推荐操作。[`activate_round`](../src/services/rounds.rs)、[`setup_round`](../src/services/rounds.rs)。

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

## 7. 结算、下单与备份

结算真源是 [`evaluate`](../src/settlement/engine.rs)。目录仅子项调价，调价不参与折扣/减均/特典档位；规则只试算与显式独立保存，不顺带保存订单。

排位/权限/目录/称呼/规则变化，旧订单保留但提示需重新核对、旧来源试算。普通后台保存不能清掉标记；管理员显式保留当前订单复核才记录新来源基线。锁定前检查数量、目标/价格和唯一订单号，问题可定位实际输入。重排包单独显式触发；正式页面不注入演示订单。[`put_draft`](../src/services/settlement/mod.rs)、[`lock_draft`](../src/services/settlement/mod.rs)，来源守卫见 [结算模块](../src/services/settlement/)。

私有导出/预检/显式应用支持完整日志和人工选择，保留当前配置/订单草稿；恢复可重试，不重注入NC或请求模型。备份不能上传公开Issue/PR；浏览器下载形态仍是差距。[`snapshot_export`](../src/services/snapshot.rs)、[`snapshot_import`](../src/services/snapshot.rs)。

## 8. 测试与仍未完成项

可重复检查见 [DEVELOPMENT](./DEVELOPMENT.md) 与 [桌面套件](../tests/e2e/README.md)。桌面仅验收1920×1080、100%、DPR1；计数以实际输出/台账为准，不按总数推断业务全覆盖。私有样本与源表只读，输出在隔离目录；缺私有输入不声称语料通过。真实接收器/真实模型/公网发布不属于自动测试。

`simulate` 已经从旧ReplayEngine切到当前确定性会话：[`run_cli`](../src/simulation/verifier/mod.rs)、[`replay_prepared`](../src/replay/session/mod.rs)。只读夹具显式提供目录/容量、阶段/优先、CN/别名，不加载真实配置、缓存或模型；当前夹具用 `fixture_version:2`，缺省1保留历史夹具的未支持改单理解，不给旧语料新增动作。两种版本都用同一准备/权限/取消/分配逻辑，旧执行器及仅它使用的领域壳已删除，未恢复旧serde键。

消息、每步/最终排位与结算显示CN；无CN按对应步骤最新有效称呼回退，重复/Drop不污染当前名。报告版本按实际事件数从0起，取消也增长；input_message_count与最终结算不再返回旧默认空值。步骤diff按商品/款式/盒/格比较，包含撤空消失格、同人换认购/状态变化，顺序固定；款式差异附可选subitem_id。原result/outcomes/report与步骤字段保留，新增Dropped/Merged/Error计数。格式与验证见 [DEVELOPMENT](./DEVELOPMENT.md)。

管理员目标改单语法、完整具名代理歧义、有界入站、调度可观测性及跨商品提示分级等仍以 [TODOS](./TODOS.md)/工程计划为准；ColumnLocked与管理员阶段豁免仍需业务决策。远程发布暂停，未部署或重启常驻实例。

## 9. 已放弃／历史内容（不可据此重新执行）

2026-09-14调查与之后分批记录完整移至 [历史快照](./archive/FUNCTIONAL-2026-09-14.md)。其旧行号、52/8测试、已删除的pipeline.rs/ws_server.rs等路径，以及“复盘501”“结算未接线”“无持久化/需主动恢复”“serve走旧栈”“模拟器仍可达”等描述不再有效。

不恢复：日常改原始消息、批量LLM重解析、覆盖整文件式撤回、按事件数/seq判旧行更新、默认开启merge、删除持久锁或接受伪造管理员提权。修正动作完整替换，原自动理解与后续操作保留；这些与旧提案冲突时以DECISIONS §H和现行消息计划为准。共享历史强推与公网发布需独立授权，不在当前自动执行范围。
