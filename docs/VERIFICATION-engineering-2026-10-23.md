# 后继工程有限验收台账（2026-10-23）

> 对应[工程计划](./PLAN-engineering-followup-2026-10-19.md) E0…E4、T-14…29和有效UP条款。不是全程序形式化证明，也不将历史SPEC整篇当作需求。基线B2为7be4ff2（G、已推送）；本批安全补强及完整门禁已通过，签名提交/远程同步由交付时实际Git核对。
> 仅占位隔离环境，无真实群/配置/接收器/模型操作；不部署或重启常驻实例，不修改只读语料，不重写共享历史。

## 1. 有效条款 → 代码 → 直接证据

各用例均由统一入口实际执行，不把“文件/测试存在”当作已经运行。MP41＋IA18的产品验收沿用[原专项台账](./VERIFICATION-msg-ia-2026-10-18.md)，本批完整桌面回归防止工程改造破坏原交互。

| 项目 | 现行合同与源码 | 直接证据（测试/原实施批次） |
| --- | --- | --- |
| E0审计 | [计划§1–3](./PLAN-engineering-followup-2026-10-19.md)、[TODOS](./TODOS.md)区分现行/旧冲突/待定/暂停 | 本台账有限范围；旧合并默认开、删持久锁、删原消息步、重签强推均不实施 |
| E1 / T-20 | [隐私守护](../scripts/privacy-scan.mjs)同时检查索引/工作副本与已知成员/配置来源 | [21类自测](../tests/privacy-scan.mjs)：card/CN/别名/自定义缓存/轮次、坏源、示例不能豁免真实值、vendor密钥、索引与工作树差异；计划§4 |
| E2 / T-21 | [隔离入口](../scripts/check.mjs)、[环境/失败边界](../scripts/check-lib.mjs)、[文档验证](../tests/docs.mjs) | [8入口用例](../tests/check-runner.mjs)含真正链接失败必须停止、不泄露私有诊断；公开结算参考确实另跑1项，遗漏私有输入为NOT RUN；计划§5 |
| E3 / T-24 / W-G2-05 | [verifier适配/执行](../src/simulation/verifier/execute.rs)→[当前会话](../src/replay/session/mod.rs)；无旧ReplayEngine | [迁移性质](../src/simulation/verifier/migration_tests.rs)逐prefix对照真实Pipeline、CN/冻结/撤改/包尾/不同容量/权限/差异；[公开CLI](../tests/offline-cli.mjs)四组226条旧签名逐格基线；计划§7 |
| T-14 | [target_modify](../src/llm/pipeline/target_modify.rs)保留真实actor/原话、唯一群成员目标，强制Modify | [19项边界](../src/llm/pipeline/target_modify_tests.rs)：目标资格、管理员/开关、阶段/锁、模型冲突、写失败与姓名、后续撤销/人工撤回；计划§11 |
| T-16 | [私有原字节传输](../src/services/snapshot/transfer.rs)、[HTTP边界](../src/api/snapshot_transfer_routes.rs) | [7项实HTTP](../src/llm/pipeline/snapshot_download_tests.rs)含Origin/64MiB/大整数、writer等候后重验、坏源/锁/过期不应用；[真实浏览器](../tests/e2e/private-backup.mjs)文件选择/预检/明确应用；计划§12 |
| T-17 | [replay_diff/do_replay](../src/services/replay.rs)当前实例切面、无隐含最后预览 | [8项实测](../src/llm/pipeline/replay_diff_tests.rs)：新消息/同轮跨实例不串、显式预览不改变GET、未初始化不恢复、坏源/积压失败；计划§13 |
| T-18 / T-19 | [成员来源与缓存安装](../src/services/members.rs)、[单飞/日历健康](../src/services/member_pull.rs) | [成员守卫](../src/llm/pipeline/member_tests.rs)及计划§9六项来源/失败/并发/取消/日历证据，真实桌面不覆盖草稿；实机NapCat不在测试范围 |
| T-22 | [现有公开模板](../config.example.json)与[配置测试](../src/settings/tests.rs) | 不另造不一致结算样例；公开参考和桌面实际读取模板，结算整数分/减均仍回归 |
| T-23 | [持久接收与worker](../src/gateway/ws_server/intake.rs)、[合同](./SPEC-DURABLE-INTAKE.md) | [256帧/echo/FIFO/故障](../src/gateway/ws_server/intake_tests.rs)、[绑定/收据/部分提交](../src/llm/pipeline/ingress_tests.rs)；计划§8。只保证有界交接内存，不保证有限磁盘或物理断电恰好一次 |
| T-25 | [纯目录诊断](../src/services/rounds/check.rs)、verifier分层、snapshot构建/恢复/传输分层，控制导出共用完成切面 | [目录有限矩阵](../src/services/rounds/catalog_tests.rs)、[控制导出](../src/llm/pipeline/export_command_tests.rs)与真实WS收据、入口实际失败诊断；计划§6/14/15，不以移动文件冒充业务迁移 |
| T-29 | [唯一成员索引](../src/settings/targets.rs)、[代理分段](../src/llm/pipeline/proxy.rs) | [代理交叉回归](../src/llm/pipeline/proxy_tests.rs)：群内乙资格/归属乙、群外代号按甲、同名/部分名/错群不猜、旧无actor不迁移、冻结后不重指；计划§10 |
| UP-01 | [阶段事件](../src/llm/pipeline/state.rs)和[持久结算锁](../src/services/settlement/mod.rs)继续独立存在 | [序列化/锁故障](../src/llm/pipeline/boundary_tests.rs)、[锁后拒绝不复活](../src/llm/pipeline/publication_tests.rs)、[启动恢复](../src/llm/pipeline/computed_tests.rs)；不按缓存解锁 |
| UP-02 | [私有computed](../src/replay/computed/mod.rs)完整来源/构建/内部资格/称呼/哈希，严格源先于缓存 | [9项真实管线](../src/llm/pipeline/computed_tests.rs)与[内部边界](../src/replay/computed/tests.rs)：真实hit、跨实例、旧条同seq/同mtime换名、坏源/超限/格式/写失败、cold/parked/fresh/replay；计划§16 |
| UP-03 | [提交后发布](../src/llm/pipeline/live.rs)共用推进；[控制](../src/llm/pipeline/admin.rs)副作用前授权并冻结结果 | [9项发布](../src/llm/pipeline/publication_tests.rs)：8分支逐prefix全字段、完整checkpoint hit、控制重试/导出切面、已提交后故障模型仅一次、缓存写失败不回滚；计划§18 |
| UP-04 | [同一compute的组前缀备忘](../src/replay/session/incremental.rs)，首差异起整个后缀 | [7项性质](../src/replay/session/incremental_tests.rs)与[3项真实管线](../src/llm/pipeline/incremental_tests.rs)：129对1实际求值、旧条/删除/乱序/人工选择/冻结组、称呼/资格、fresh cut；计划§17 |
| UP-05 / F1 | [配置共同提交](../src/services/configuration.rs)、[setup/activate](../src/services/rounds/mod.rs)、[测试注入](../src/api/sim_routes.rs) | [safety_category_binding…](../src/llm/pipeline/safety_tests.rs)：两种错配、配置PUT、setup、三种激活、正常轮单/批注入，不改变原板/配置/文件；无测试群回退。旧全库测试群号唯一性不是已实现合同 |
| UP-05 / F2 | NC门禁仍仅测试轮＋测试群＋明确开关；不改用户确认的时间及排序 | [gate_nc组合/批量排序](../src/llm/pipeline/tests.rs)、[跨轮绑定](../src/llm/pipeline/ingress_tests.rs)；旧“丢弃内层timestamp/伪造提权”不执行，管理员阶段豁免仍待定 |
| UP-05 / F3 | [共同标识验证](../src/settings/rounds.rs)与可失败路径构造，消息/审计/replacement/结算/computed均拒绝非法标识 | [底层存储矩阵](../src/messages/safety_tests.rs)、[严格启动](../src/settings/tests.rs)、[结算路径](../src/services/settlement/tests.rs)：路径/空白/Windows设备名/长度，失败无目录或回退写入 |
| UP-05 / F7 | 同一MessageLog writer保护运行时RMW/投影与轮次创建/删除；配置双文件和日志replacement有独立恢复协议 | [并发新入站/两编辑者](../src/llm/pipeline/acceptance_tests.rs)、[配置准备/锁并发](../src/llm/pipeline/boundary_tests.rs)、[配置落盘故障](../src/settings/commit.rs)、[导入故障](../src/llm/pipeline/snapshot_tests.rs)；新[两项轮次用例](../src/llm/pipeline/safety_tests.rs)证明坏文件/Windows大小写别名不覆盖、同ID创建只成功一次、锁内等待及等候后不能删新激活轮。不宣称外部进程/跨系统原子性 |
| UP-05 / F8 | [OneBot规范化](../src/gateway/onebot/mod.rs)4096字符普通文本；JSON不截断，WS帧/消息1MiB；普通HTTP默认2MiB、私有备份64MiB | [Unicode/JSON保真](../src/gateway/onebot/tests.rs)、[实际WS超限](../src/gateway/ws_server/intake_tests.rs)、[HTTP精确上限/超限零副作用](../src/llm/pipeline/safety_tests.rs)、原64MiB实HTTP测试；没有把所有入口伪称统一4096字符 |
| UP-05 / F9/F10 | [原帧与内部审计解码](../src/messages/mod.rs)区分来源，同轮/类型检查；全算/增量/缓存/导入共用 | 实际红测普通发言携带RoundPhaseSet会被拒绝并锁轮；[管线来源](../src/llm/pipeline/safety_tests.rs)、[Drop网关帧](../src/gateway/ws_server/intake_tests.rs)、[阶段/override类型矩阵](../src/messages/safety_tests.rs)修复验证。历史OneBot post_type不得作控制，完整原JSON仍可查看/备份 |
| UP-06 | 原消息/组内每步仍存在，三视图与追溯可达，不折叠掉组内消息 | [fresh增量真实桌面](../tests/e2e/incremental-fresh.mjs)和原桌面三视图/游标/草稿/原位修订回归；冷恢复首条可完整热身，源读取和分配非O(1) |
| UP-07 / T-26 | 本台账逐项对照，加完整公开/私有只读语料、桌面、隐私和文档门禁 | 最终实际结果填§3，不靠测试计数宣称全状态/全路由覆盖 |

## 2. 明确不属于本目标的未完成项

- W-G2-06 ColumnLocked、T-28管理员阶段豁免：业务待定，不自行定义/删除、不绕过持久锁。
- T-27远程发布/部署、真实运行迁移及重启：用户暂缓或未授权，不以开发完成自动执行。
- UP-08历史隐私清洗/重签/force-push：需独立范围、备份和协作者协调；本目标只做新增日常G签名提交。
- 私有备份不含独立成员缓存、浏览器未保存草稿或待接收队列，不可当公开分享物或整个接收器灾备。未知隐私/DLP、真实NapCat兼容和任意断电可靠性不作认证。
- 热身备忘仅内存、部分fresh cut不持久化；控制副作用在控制收据提交前仍有跨文件失败窗口。它们是明确工程合同，不是用缓存或总数隐藏的完成功能。

## 3. 本批最终门禁

最后一轮完整入口退出码0：472核心＋1公开结算参考＝473项Rust；35项桌面全套（1920×1080、100%、DPR1）、8入口自测、3文档检查、21隐私自测及305暂存/跟踪文件扫描全部通过。格式、严格clippy和build通过；公开CLI三条及四组226条完整格/空位/容量/策略基线，两套明确私有样本与2工作簿67消息/47分配组全部通过，源哈希不变。此后只整理文档，文档/隐私再复验，不把旧代码的证据代替新源码门禁。

本批新增13项有限安全用例，覆盖轮次标识/底层存储/启动来源/结算、Unicode/JSON长度、真实WS及HTTP限额、轮群配置/setup/激活、原帧阶段/override来源、Drop帧、创建/删除共同writer及同名源保留。实际先红后绿，不放宽旧整数/板/原消息/草稿断言；封装后的WS导出仍检查9007199254740993精确整数，并保留查看原JSON的对照。一次链接LNK1104严格停止、未运行测试不计红测或通过，原样重试后两个轮次用例才实际红测。

E0…E4和T-26/UP-05/UP-07的有效范围在本台账内验收完成；§2仍保持待定/暂缓/单独授权，不冒充已实现。没有部署/重启41688、操作真实群/配置/模型、改变UI样式或修改只读语料；本轮隔离产物已由入口清理，真实数据未删。旧重复启动回退/宽松落盘代码删除仅属仓库源码，可从Git恢复。
