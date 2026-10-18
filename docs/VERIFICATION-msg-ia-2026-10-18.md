# 消息专项与桌面整并 · 逐项验收台账

> 2026-10-18 建立。用户已明确授权持续目标：完成 MP-0…6、IA G0…G5 及全部隔离测试、签名提交推送。基线 b8be8eb；未部署。历史阶段通过不等于本台账所有用例通过。
> 仅 1920×1080、100%、DPR=1；占位数据和隔离目录；无真实模型、真实群动作或配置。截图只留本机，不公开私有数据。

> 2026-10-19 收口：本表41＋18项均已逐行登记可重复证据；最终核心319、桌面完整30项、两套只读语料、246文件隐私扫描通过。各批历史“未完成”保持历史语境，不再作为当前状态。签名/远程交付以本提交实际 Git 签名和上游相等为准；未部署。

## 本轮逐项复验范围（2026-10-19）

以下每行列出实际测试，不按测试总数推算覆盖率。`pipeline` 测试均位于 `src/llm/pipeline/`，浏览器测试均位于 `tests/e2e/`；同一用例可由核心与浏览器两种证据共同闭环。

故障恢复采用可重复的写入边界故障注入、重新打开存储/管道与真实文件热载；不宣称验证硬件强断电。浏览器中文组合输入通过 Chromium IME 协议，粘贴使用受控 ClipboardEvent＋原生插入，不读取/覆盖系统剪贴板。真实 NapCat/模型、生产迁移和部署不在本验收范围。

## 消息专项（41 项）

| 用例 | 场景 | 状态 | 可重复证据 |
| --- | --- | --- | --- |
| MP-T01 | 实时/drop/管理插入造成 seq 分歧，再修订 | 通过 | message-identity：真实管理插入＋非接收群 Dropped 后确认旧条，仅目标变化；tests::live_message_identity_matches_persisted_record_before_and_after_recompute |
| MP-T02 | 重复 seq/message_id、不同轮次/实例 | 通过 | messages::tests::duplicate_seq_updates_exactly_one_record；tests::record_bound_override_does_not_change_another_message_with_same_upstream_id / override_guards_round_revision_and_exact_record_identity；closure_tests 的外实例记录拒绝 |
| MP-T03 | 完整替换商品/数量/策略/多行 | 通过 | closure_tests::complete_multiline_strategy_selection_survives_restart_and_later_modify：单领/拼团、包尾/整盒、稳定多行与备注；message-actions：多动作实际保存 |
| MP-T04 | 自动结果已正确，新增管理员解释 | 通过 | transaction_tests::projection_cursor_detects_old_note_quantity_and_policy_edits_without_event_count_change；message-policy-return：note-only 保存，原动作/事件数不变、复盘新解释可见 |
| MP-T05 | 自动 Ignored/NeedConfirm/Error，人工给完整动作 | 通过（第十二批核心复验） | acceptance_tests::mp_t05：真实忽略/待确认及占位 Error 理解产物分别改为完整动作，原产物不变、2 份实际生效、模型 0 次 |
| MP-T06 | Applied→人工忽略/拒绝→重启/重算 | 通过 | action_tests::typed_ignore_reject_empty_and_restore_are_distinct_persistent_choices；transaction_tests::undo_reopens_from_a_fresh_pipeline_and_restores_previous_manual_choice；message-actions 逐一保存三种选择 |
| MP-T07 | 恢复自动或上一人工版本，LLM 禁用 | 通过 | 同 T06；transaction_tests 的 fresh-pipeline 撤回恢复上一 Ignore，不伪造自动结果；automatic-baseline / message-actions 实际恢复 |
| MP-T08 | 排/撤/改、单领/拼团/包尾及动作行排序 | 通过 | action_tests::typed_multi_action_order_and_ids_survive_reordering_without_duplicate_execution；closure_tests 的多类型多行重启保真与后续 Modify；message-actions 的可见位置策略/多动作保存 |
| MP-T09 | 仅改优先名单，原 Rejected 变 Applied | 通过 | automatic_tests::rejected_llm_actions_revalidate_priority_without_a_second_model_call；boundary_tests::actual_file_watch_uses_runtime_and_recomputes_external_priority_change；automatic-baseline 的真实 UI 保存 |
| MP-T10 | 从权限拒绝跳设置保存后返回 | 通过 | message-policy-return：实际跳优先设置、全轮零模型重算、原草稿/筛选/滚动返回、一次保存及记录冲突只读保留 |
| MP-T11 | 很久后的补充性解释 | 通过（第十二批核心复验） | acceptance_tests::mp_t11_t29：45 天后两条补充，主条完整替换/补充拒绝，同一事务；原消息字段/数量不新增 |
| MP-T12 | 不连续补充，中间有他人消息 | 通过 | transaction_tests::atomic_supplements_keep_intervening_other_member_and_undo_after_new_messages；message-transactions：两条同人夹他人，完整替换只执行一次 |
| MP-T13 | 主条与补充条原先都产生动作 | 通过 | 同 T12，原数量 1＋2 改为主条 3、补充 Rejected；他人 2 保留，总数 5；解除/撤回恢复原选择 |
| MP-T14 | 人工选取落在自动聚合组中 | 通过 | transaction_tests::automatic_group_conversion_and_undo_preserve_post_edit_units / transaction_rejects_partial_auto_groups_duplicate_and_reverse_supplements；suggest_tests::partial_automatic_unit_is_rejected_before_model_and_complete_unit_is_draft_only；message-transactions 实际整组转换 |
| MP-T15 | 循环、重复吸收、跨轮/实例/群 | 通过 | closure_tests::supplementary_cycles_reuse_foreign_groups_and_instances_cannot_commit；action_tests 的 stale/round 守卫；事务测试拒绝半组/倒序/重复，失败日志不变 |
| MP-T16 | 修改后又有相邻实时发言 | 通过 | transaction_tests::automatic_group_conversion_and_undo_preserve_post_edit_units；automatic_tests::merge_setting_changes_do_not_repartition_persisted_automatic_units；message-transactions 保存后真实新撤排再撤回 |
| MP-T17 | 动作数不变但内容/解释/权限变化 | 通过 | transaction_tests::projection_cursor_detects_old_note_quantity_and_policy_edits_without_event_count_change；message-policy-return 同事件版本刷新复盘，步骤/查询保留 |
| MP-T18 | 旧行分页外、断线/游标过期/重启 | 通过 | closure_tests::historical_message_outside_live_projection_can_be_opened_corrected_and_resynced：历史详情回退、重启 epoch replace；message-identity：断开 API 时划词/节点保留，离线期旧条修订，恢复原行/原滚动；服务端不以旧 since 隐去修订 |
| MP-T19 | 新入站、双编辑、配置热载并发 | 通过 | acceptance_tests::mp_t19：join 双编辑＋新入站，无丢条；boundary_tests::actual_file_watch 真文件热载；suggest_tests::pending_draft_rechecks… 等待中主条/补充/配置/新组/锁变化，写锁不被模型占用 |
| MP-T20 | 锁中/保存时锁定、换轮/实例 | 通过 | boundary_tests::locking_and_manual_save_are_serial_and_durable_lock_failures_never_claim_success / continue_round_switches_keep_own_cached_state_and_never_previous_round_data；draft-concurrency：换轮/锁中响应不入草稿，锁后打开只读；稳定记录/实例匹配拒绝 |
| MP-T21 | 写入/重算失败，提交各阶段崩溃 | 通过（边界故障注入） | transaction_tests::journal_write_failures_are_atomic_and_lost_response_retries_recover / corrupted_atomic_choice_cannot_publish_valid_siblings_or_revive_prior_actions；boundary_tests 的配置准备失败；settings::commit 的三边界恢复；snapshot_tests::private_import_is_recoverable_at_all_file_boundaries_and_blocks_interleaved_writes；不声称硬件断电测试 |
| MP-T22 | 重复点击/断线重发同保存 | 通过 | action_tests::typed_save_is_idempotent_but_reusing_key_for_other_content_is_conflict；transaction_tests::journal_write_failures_are_atomic_and_lost_response_retries_recover；message-identity 真实重复请求返回 deduplicated、不同事务旧版本 409 |
| MP-T23 | 早期 Claim 纠错后，后续 Cancel/Modify | 通过 | action_tests::typed_early_edit_preserves_later_cancel_and_other_members；closure_tests::complete_multiline_strategy_selection_survives_restart_and_later_modify；T40/41 保留原数量与商品条件 |
| MP-T24 | 滚动/划词/键盘/设置往返/筛选移出 | 通过 | message-identity：Tab/Escape/Enter、旧行滚动、断线划词；message-policy-return：携草稿、后退、筛选外暂留/显式清除；message-actions：长模态滚动与冲突保留 |
| MP-T25 | 修订/政策重算后复盘与结算 | 通过 | message-policy-return 真实 note-only 修订刷新当前轮复盘且第11步保留；settlement_tests::upstream_changes_require_explicit_review_without_erasing_orders_or_calling_model；settlement-review 权限/撤排变更→保留订单→复核/差额定位 |
| MP-T26 | 捕获全部模型调用与外部动作 | 通过（隔离 mock） | CountingLlm 捕获完整提示/次数，启用模型后保存/撤回/权限重算为0；suggest_tests 显式代填仅1次且最小上下文；message-transactions 假接收器捕获全部出站应用帧为0；真实模型/NapCat未接入 |
| MP-T27 | 保存后立即撤回，无其他变化 | 通过 | transaction_tests::undo_reopens_from_a_fresh_pipeline_and_restores_previous_manual_choice；message-transactions 整组保存后立即撤回，恢复原3份，不产生 redo |
| MP-T28 | 保存后收到新消息/撤排，再撤回 | 通过 | transaction_tests::atomic_supplements_keep_intervening_other_member_and_undo_after_new_messages / automatic_group_conversion_and_undo_preserve_post_edit_units；message-transactions 保存后新撤排保留 |
| MP-T29 | 主条和多条补充合并后一次撤回 | 通过（第十二批核心复验） | acceptance_tests::mp_t11_t29：主条＋两补充一起恢复，关联清除、原 Ignored/Applied 恢复，记录版本递增、两笔审计追加；模型启用仍 0 调用 |
| MP-T30 | 后续人工事务/版本冲突/锁使撤回过期 | 通过 | transaction_tests::failed_save_does_not_consume_undo_slot_and_new_manual_transaction_expires_old_token / undo_rejects_changed_policy_phase_and_corrupted_input_without_rollback；锁态 undo_status 禁用 |
| MP-T31 | 重启后撤回；上次保存失败 | 通过 | transaction_tests::undo_reopens_from_a_fresh_pipeline_and_restores_previous_manual_choice / failed_save_does_not_consume_undo_slot_and_new_manual_transaction_expires_old_token；失败不消费有效槽 |
| MP-T32 | 动作重排/删除、精确与商品条件撤销 | 通过（第十二批核心/浏览器复验） | action_tests::typed_multi_action_order…＋acceptance_tests::mp_t32：失效精确引用不改撤其他单、逐项/数量/商品范围守卫；message-actions 实际界面显示未命中及保留失效下拉项 |
| MP-T33 | 当前动作只改一个字段并保存 | 通过 | action_tests::typed_quantity_edit_keeps_raw_and_primary_claim_identity_and_replays_after_restart；message-actions 数量初值1→3，一次保存，原自动/原文保留 |
| MP-T34 | 一句解释代填后直接改数量，再保存 | 通过 | suggest_tests::explicit_draft_calls_model_once_but_never_saves_or_executes：代填3→手改4→保存，模型计数仍1；message-transactions 显式填草稿而未执行 |
| MP-T35 | 等待代填时手改、换记录/轮次、连续请求 | 通过 | message-transactions 手改阻止迟到覆盖；draft-concurrency 取消、两个请求逆序返回、关闭后开另一记录、换轮、并发修订、锁定均不误填 |
| MP-T36 | 模型关闭/超时/错误 JSON/歧义 | 通过 | suggest_tests::unavailable_malformed_or_unsafe_draft_keeps_manual_choice_and_journal；draft-concurrency 的504、坏JSON、受控传输超时，手工输入/保存仍可用；空动作不猜忽略 |
| MP-T37 | 观察显式代填请求范围及返回目标 | 通过 | suggest_tests 最小提示排除名单/群号/raw与身份字段，非法撤销目标拒绝；完整单元/补充版本与配置/锁终检；draft-concurrency 记录/请求/草稿版本和本机上下文交付守卫 |
| MP-T38 | 打开/展开/打字/保存/撤回/权限重算 | 通过 | draft-concurrency 仅显式点击增加请求；message-policy-return 捕获请求无 suggest；核心 CountingLlm 启用时保存/撤回/重算0次；message-transactions 假网关出站0帧 |
| MP-T39 | 同人紧跟“排 A→撤 A”，自动合并开/关各一次 | 通过（第十二批核心复验） | acceptance_tests::mp_t39：merge 开关各一次，两个理解单元/原位记录、数量归零、模型 0 次 |
| MP-T40 | 甲排 A 获 B 位，中隔本人其他商品/闲聊及他人排 A，再撤 A | 通过（第十二批核心复验） | acceptance_tests::mp_t40_t41：保留本人其他商品/他人 A，仅撤本人 A；merge 开关均验证，自动组修订完整选择范围 |
| MP-T41 | 原位修正甲早期 A 数量，保留后来的撤 A | 通过（第十二批核心复验） | acceptance_tests::mp_t40_t41 / mp_t41_explicit_product_scope…：原数量/条件的后续撤销继续生效，显式其他商品及同名子项范围不偷改，原时间保留 |

## 信息架构与交互（18 项）

| 用例 | 场景 | 状态 | 可重复证据 |
| --- | --- | --- | --- |
| NAV-01 | 7 个工作区逐一点击其余 6 个入口 | 通过 | workspace-navigation：42 对实际点击、自跳不重载、顺序/高亮/API/轮次一致 |
| NAV-02 | 页内子项、刷新、后退/前进、直接打开 hash | 通过 | workspace-navigation：四设置分区分别直开/刷新、实际后退前进、唯一面板/当前标记；message-policy-return 后退恢复原任务/位置；#order 高亮一致 |
| NAV-03 | `admin.html`、`/admin`、短路由、`/web/*.html`、旧成员/消息锚点 | 通过 | workspace-navigation：三个 admin 兼容、14个短/旧路径、旧消息锚点、未知hash提示后点击恢复；correctness-gates 旧成员锚点到独立真源 |
| CTX-01 | 自定义 API、不同本机偏好、远程展示、导入回放间跳转 | 通过 | workspace-navigation：过期本机偏好不覆盖同源、自定义API跨42对保持、拦截外实例请求0；message-policy-return 导入含真实占位ID仍只读、跨轮不激活、远程不编辑；draft-concurrency 换轮迟到拒绝 |
| CTX-02 | 草稿未保存时切页/轮/实例；页面重载 | 通过 | scoped-settings：保存/放弃/取消、取消激活轮次/换实例不发命令、beforeunload拒绝重载保留；correctness-gates 成员来源独立草稿；message-actions 消息草稿保护 |
| CFG-01 | 成员、接入、LLM、展示、结算逐区保存并重载 | 通过 | scoped-settings 各区保存/并发同区冲突；correctness-gates CN/来源隔离；settlement-review 规则独立保存保留并发显示设置/订单；member-boundaries 读取与建议迟到不覆盖人工输入 |
| CFG-02 | 逐字输入、粘贴、中文输入法、清空重输、恢复自动 | 通过 | correctness-gates：逐字、Chromium真实组合输入、受控粘贴、清空重输保留焦点、保存完整称呼、恢复自动删除冻结CN；无系统剪贴板访问 |
| CFG-03 | 商品存在整盒关联，编辑其他字段并保存 | 通过 | correctness-gates：box_item_id 与显式 class B 保存/重读保留、控件业务名称可达；实际订单/排位引用原商品，不改目录归属 |
| MSG-01 | 实时消息+管理插入+drop+重算后人工确认 | 通过 | message-identity 的实际管理插入/Dropped/旧条确认及稳定record版本；见 MP-T01/02/18 |
| MSG-02 | 动作句/可选代填、override、补充性合并、权限设置往返与单步撤回 | 通过 | 本表 MP-T01…41；message-actions / message-transactions / message-policy-return / draft-concurrency 与各核心专项共同闭环 |
| ORD-01 | 存在未认领商品，生成订单、填号、锁定/解锁 | 通过 | settlement-review 真实点击生成→逐包填号→锁/解锁；未认领零价项不阻断；缺号/重号/价格/差额按钮实际聚焦，失效来源保留；correctness-gates 锁后禁止移动 |
| ORD-02 | 鼠标拖动与“移动到包…”分别执行同一搬移 | 通过 | correctness-gates：真实 dragTo 和下拉按钮产物完全相同，数量/金额及草稿重读保真；锁后拖动/替代移动禁用 |
| A11Y-01 | 只用 Tab/Shift+Tab/Enter/Space/Escape 完成代表任务 | 通过（代表路径，非全面WCAG认证） | message-identity Enter/Tab/Escape/焦点返回；scoped-settings Shift+Tab/三选保护；settlement-review Enter/Space 问题定位；replay-graph Space 选步/列、键盘定位且不遮挡 |
| A11Y-02 | 打开/取消/提交/错误时关闭模态 | 通过 | message-identity 初始焦点/Tab约束/校验错误/提交；message-actions 长体滚动、关闭取消/放弃及409保留；scoped-settings 二层草稿模态Escape/焦点恢复；draft-concurrency 关闭取消待发请求 |
| VIS-01 | 长表首/中/末、分类切换、锚点、横向滚动 | 通过 | catalog-reachability：四类/明暗/筛选及目录首中末实际点击、焦点和 elementFromPoint 未遮挡；board-layout 超宽内容真实横滚首/中/末，无整页溢出；message-actions 长模态滚动；replay-graph 定位底部且吸顶不遮焦点；截图人工复查 |
| VIS-02 | 三字/全长、不同盒列、主题切换、复盘切步 | 通过 | board-layout：实际字体/空位等宽、盒位≤1px、全长无截断、两主题明确设定后等待过渡完成、上下标记2/4px、全历史定宽/步骤/高亮时效保留、跨页/标签共享模式 |
| ERR-01 | API 断开、NapCat 未连接、LLM 失败、409/423、空数据 | 通过 | message-identity API断开保留原行/导航并重连；member-boundaries 接收器502不丢名单；draft-concurrency 模型/传输故障与423只读；message-actions 409留草稿；settlement-review 503不伪造可写空草稿 |
| SAFE-01 | 正常/测试轮与群绑定组合、只读来源、危险操作取消 | 通过 | category_issues / gate_nc 组合核心测试；批量注入缺字段/错群逐条Dropped、正常轮拒绝注入；message-policy-return 导入/远程只读；scoped-settings 取消维护/激活零命令；外发捕获0帧，真实群不接入 |

## 批次记录

- 第五批 b8be8eb：293 Rust / 22 e2e / 99 条与 46 商品语料 / 227 跟踪文件隐私检查通过。功能证据见消息计划 §18.2；本表仍须逐项核对，不能按测试总数换算完成百分比。
- 第六批：真实 42 对导航点击及旧入口/同源 API/单一编辑真源、1920×1080 明暗截图与对比度检查；23/23 e2e、293/293 Rust、fmt/clippy 通过。NAV-02/03 尚待完整草稿/未知锚点恢复验证，其余行不按总数自动打勾。G3/G4 后续不冒充完成。
- 第七批：最终 24/24 e2e、293/293 Rust、fmt/clippy/build；隔离副本重新生成输出后 99 条/46 商品通过。新增分区冲突、草稿三选、命令取消、浏览器重载、Tab/Escape、面板居中/轨道对齐的真实断言；明暗及草稿截图已人工查看。仍不是本台账整体完成。
- 第八批：最终 25/25 e2e 通过；权限设置真实往返和浏览器后退、原草稿/位置/筛选保留、原自动拒绝与零模型重算、当前复盘键盘到处理、冲突只读、跨轮/导入/远程只读。明暗截图复查、233 文件隐私与 159 个本地文档链接检查通过；不将 41+18 项全部标通过。
- 第九批（2026-10-18—19）：301/301 核心、26/26 e2e、fmt/clippy/build 和隔离语料 99 条/46 商品通过。成员建议使用 mock，关闭/过期不外发、等待中配置/名单变化拒绝、严格响应与无落盘、缓存路径/包装/数值 ID、失败原子写保留均有独立回归；浏览器确认先预览后选择/保存、等待中手改与断开保留。剩余结算/关系图和完整逐项终检仍未冒充完成。
- 第十批（2026-10-19）：306/306 核心、严格 clippy/fmt、全套 27 e2e 与额外主题稳定态/Enter/Space 定位专项通过；只读样本在隔离副本重放 99 条/46 商品通过。来源/草稿版本守卫、普通保存不算复核、显式复核保留订单、延迟保存 409 保留输入、问题下标定位、规则独立保存、坏/旧草稿、读取失败不可写均有实际证据。明暗截图在 CSS 过渡结束后取证并测订单输入对比度≥4.5，已人工查看。未将完整 MP 41 / IA 18 项自动标通过；关系图与逐条终检继续。
- 第十一批（2026-10-19）：306 核心、严格 fmt/clippy、28 e2e 通过；商品聚焦/分组、长混排全名与特殊字符、单领/等待称呼、键盘定位及不遮焦点、独立视区/切步保持、迟到图与零管理写入有新专项。Space 选择步骤/列后焦点仍在，列三字/全长回归通过。修复全局滚动偏移误用于内部吸顶控件的 194px 跳动；复盘旧消息 note-only 变化通过 workflow 同代投影游标刷新，原步骤/定位保留。明暗/长图首部及定位底部截图已查看。接下来逐项审计并补测本表全部待闭环行，不将 28 项总数映射成 59 项完成。
- 第十二批前段：8 项逐条接受测试，当前 314 核心与严格 clippy、28 e2e、隔离 99 条/46 商品通过。T05/11/29/32/39–41 已登记具体证据，其余行继续审计。新失败回归发现并修复多商品撤销只取首项、同名款式丢商品范围、明确未知商品降为最近一笔及未命中执行说明；旧产物不重解析，未改变已规范撤销数量。下一段仍补模型迟到/取消、重连与 IME/导航恢复等证据，不宣称整个目标完成。
- 第十二批后段/最终复验（2026-10-19）：319/319核心、fmt/严格clippy/构建、完整30/30桌面、99条/46商品隔离样本、67条/47组原表重建逐格比较通过；原表与语料哈希不变。代填全范围/锁/交付守卫由失败回归补齐；补真实管理插入/drop/断线旧条更新、取消/乱序/切记录或轮次、模型故障、假接收器出站0帧、IME/受控粘贴、hash历史/未知恢复、取消激活、订单生成整链及目录首中末实际点击。59行逐一审计后登记，不以测试数代替；明暗/三字全长/长图/只读模态和目录截图已人工查看。隐私扫描246文件通过，171个本地文档链接和差异格式复核通过；以此状态签名提交。所有隔离服务和浏览器已退出，仅原常驻实例保留；未部署、未接入真实群/模型。
