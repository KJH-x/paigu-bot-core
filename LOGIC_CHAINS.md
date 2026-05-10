# paigu-bot-core 逻辑链条文档

每条 = 触发条件 → 事件流转 → 受影响模块 → 最终结果

---

## 一、成员排谷 (Claim)

### 1.1 普通拼团排谷
```
QQ群成员发"排燐音吧唧2，蓝良1" → RawMessageRecord入库(幂等校验) → command_router.classify_message识别为MemberClaim
→ parser.MessageParser.parse_member_message调用LLM生成ParsedMessage(intent=Claim, items=[...])
→ validator.EventValidator.validate: 置信度≥0.65, ambiguous_parts为空
→ alias_match.resolve_item: 别名匹配(item_id精确=1000, name=900, alias=800, 包含=400, category_hint=150, kind兼容=100)
→ 唯一匹配→EventEnvelope(ClaimCreated{claim_id, items:[{item_id,quantity,claim_type,slot_policy}]})
→ engine.event_store.append写入EventStore
→ engine.replay.RebuildSnapshot: 读取全部events→排序→收集有效claims→按priority_level DESC排序→AllocationEngine.allocate
→ item_working_state.find_first_fillable_normal_slot找最早空Normal槽→fill_slot填入用户
→ 生成AllocationSnapshot{item_allocations, user_summaries, warnings}
→ SettlementEngine.settle: build_user_bills→apply_discount_rules→final_total计算
→ SnapshotService.save_and_publish: SnapshotRepo持久化→R2Publisher发布current.json
→ BotReply::Text("已记录：燐音吧唧 x2,蓝良 x1，当前版本 #128")
```

### 1.2 包尾/端盒排谷
```
QQ群成员发"排特典包尾" → ParsedClaimItem(claim_type=GiftClaim, slot_policy=TailLocked)
→ allocate_tail_locked: segment_id="tail:user:claim:0" → next_box_index开新盒
→ fill_slot填入quantity个slot(状态Filled, policy=TailLocked, segment_id=xxx)
→ mark_locked_empty把盒内剩余slot标记为LockedEmpty(不会被后来散户填入)
→ 后续普通散户继续填旧盒空位(不阻塞)
→ StateDiff: reason=TailSegmentCreated, causality=Direct
```

### 1.3 单领排谷
```
QQ群成员发"排蓝良立牌单领1" → claim_type=Single
→ allocate_single: 检查max_quantity上限, 超出部分入waiting
→ SingleAllocation{user_id, quantity, unit_price}
→ SettlementSnapshot中作为单领计价(不进box slot)
```

### 1.4 代牌排谷
```
QQ群成员发"排燐音吧唧代牌1" → is_proxy_card=true
→ ClaimLine.is_proxy_card=true(标记字段, 排队逻辑不变)
→ 前端展示"代牌"标记
```

---

## 二、撤销排谷 (Cancel)

### 2.1 撤销最近一条
```
QQ群成员发"撤" → cancel_target_hint=None, items为空
→ ClaimCancelled{target_claim_id=None, target_item_id=None, quantity=None}
→ replay.apply_cancellation: 倒序遍历claims, 找user_id匹配且非空的最近一条claim→cancel_all
→ 所有claim line quantity=0, claim.status=Cancelled
→ AllocationEngine: 被释放的slot变Empty, 后续散户自动前移(auto_forward)
→ StateDiff: reason=CancelReleased(直接), reason=AutoMovedForward(连锁), causality=Cascade
→ BotReply::Text("已撤销你最近一条排谷，当前版本 #129")
```

### 2.2 撤销指定商品数量
```
QQ群成员发"撤燐音1" → cancel_target_hint="燐音1"
→ alias_match.resolve_item: item_id=badge_rinne, quantity=1
→ ClaimCancelled{target_item_id=badge_rinne, quantity=1}
→ replay.apply_cancellation: 从最后往前找user_id匹配的claim, cancel_item_quantity减少1
→ 若该claim变成空→status=Cancelled
→ 连锁前移: 被释放slot后续用户前移
```

### 2.3 管理员撤销指定用户
```
管理员发"/撤销 用户A 燐音2" → 管理员权限校验→AdminAllocationAdjusted{RemoveUserItem{user_id=A, quantity=2}}
→ engine.apply_admin_constraints→remove_user_quantity: 遍历slots清除该用户的Filled槽
→ 不做连锁前移(管理员显式移除)
```

---

## 三、优先权 (Priority)

### 3.1 优先权用户插队
```
管理员配置: Eligibility{user_id=B, priority_level=10, scope:{item_ids:[badge_rinne]}, valid_until=xxx}
→ 普通用户A在t=100排燐音1 → fill slot1
→ 优先用户B在t=101排燐音1 → EffectiveClaimLine.priority_level=10
→ sort: priority_level DESC(10>0) → B排在A前面
→ AllocationEngine: B填入slot1, A被挤到slot2
→ StateDiff: slot1加入reason=NewClaimFilled(属于B), slot2变更reason=AutoMovedForward(属于A)
→ DecisionTrace.priority_trace: {user_id:B, priority_level:10, matched_eligibility_ids:[xxx]}
```

### 3.2 优先权过期
```
优先权valid_until=2026-05-08T20:00:00Z
→ 用户B在t>valid_until时排谷 → priority_level=0(普通)
→ 不插队, 按时间戳正常排
```

### 3.3 优先权限定商品
```
Eligibility{user_id=B, scope:{item_ids:[badge_rinne]}}
→ B排燐音时priority_level=10(高)
→ B排蓝良时priority_level=0(普通, 因为scope不包含蓝良)
```

---

## 四、管理员命令 (Admin)

### 4.1 开团
```
管理员发"/开团 标题=ES5月新谷" → command_router识别AdminCreateRound
→ RoundService.create_round: Round{round_id=UUID, title, status=Active/Scheduled, group_id, created_by, allow_cancel=true}
→ RoundRepo.insert入库
→ BotReply::Text("已创建团 ES5月新谷")
```

### 4.2 添加商品
```
管理员发"/加商品 名称=燐音吧唧 类型=拼团 单价=45 盒规=10" → AdminAddItem
→ AdminService.add_item: Item{item_id=UUID, kind=Split, unit_price=4500, box_size=10, aliases=["燐音","rinne"]}
→ ItemRepo.insert入库
```

### 4.3 设置优惠
```
管理员发"/设置优惠 满300-50，购物金80" → AdminSetDiscount
→ LLM解析: [ThresholdDiscount{threshold=30000, discount=5000}, ShoppingFund{amount=8000}]
→ AdminService.set_discount_rules: DiscountRulesSet事件写入EventStore
→ settlement_engine.apply_discount_rule: 按用户金额比例分摊(最大余数法)
→ 满减: scoped_total/30000 * 5000, 按金额比例分到每个user.bill.discount_share
→ 购物金: 8000按金额比例分到discount_share
→ SettlementSnapshot更新各用户应付
```

### 4.4 满减赠品
```
管理员发"/设置优惠 满500送特典1张，特典估30，按金额比例摊" → AdminSetDiscount
→ GiftByThreshold{threshold=50000, gift_quantity_per_threshold=1, gift_valuation=3000}
→ settlement_engine: gift_count = scoped_total/50000, total_gift_value = gift_count*3000
→ 赠品估值按金额比例分摊到user.bill.gift_value_share
→ final_total = gross - discount_share - gift_value_share + shipping
```

### 4.4 锁位
```
管理员发"/锁位 商品=燐音吧唧 盒=2 位=5" → AdminLockSlot
→ AdminSlotLocked{box_index=2, slot_index=5, reason=xxx}事件写入
→ AllocationEngine.apply_admin_constraints: slot状态=LockedEmpty, policy=AdminFixed
→ 普通用户不能占用该槽
```

### 4.5 管理员固定用户
```
管理员发"/修正 商品=燐音吧唧 用户=A 盒=1 位=3" → AdminAllocationAdjusted
→ FixUserToSlot{item_id, user_id=A, box_index=1, slot_index=3}事件写入
→ admin_fix_slot: slot状态=AdminReserved, policy=AdminFixed
→ 若该槽本有其他用户→该用户被移出并重新找空位
```

### 4.6 结团
```
管理员发"/结团" → AdminCloseRound
→ RoundService.close_round: round.status=Closed, RoundClosed事件写入
→ 结团后普通用户claim拒绝(ValidationError: RoundNotActive)
→ 管理员仍可调整(AdminAllocationAdjusted)
→ ExportService.export_user_bills输出CSV
```

### 4.7 导出
```
管理员发"/导出" → AdminExport
→ ExportService.export_user_bills(snapshot): CSV格式: 用户ID, 昵称, 商品明细, 原价, 优惠, 赠品抵扣, 邮费, 最终应付
→ ExportService.export_item_summary(snapshot): CSV格式: 商品ID, 商品名, 类型, 数量, 原价合计, 已成盒数, 未成盒数, 赠品数量
```

---

## 五、解析与校验 (Parser & Validation)

### 5.1 LLM解析成功
```
成员消息文本 → prompt.build_system_prompt系统提示词 + ParseRequestContext{group_id, user_id, nickname, message, active_rounds}
→ llm_client.LlmClient.parse_message → LlmParseResponse{raw_text, parsed:ParsedMessage, model}
→ ParsedMessage{intent, round_hint, items:[ParsedClaimItem{name, quantity, claim_type, slot_policy}], confidence, ambiguous_parts}
```

### 5.2 解析失败(JSON反序列化)
```
LLM返回非JSON → parsed_messages.status=failed, error="Invalid JSON"
→ BotReply::Text("没有识别成功，请按格式重发")
```

### 5.3 置信度不足
```
LLM返回confidence=0.50 < threshold=0.65
→ ValidationOutcome::Reject("识别置信度不足(50%)，请按格式重发")
→ 不落事件
```

### 5.4 商品歧义
```
LLM识别"燐音" → alias_match.resolve_item: "燐音吧唧" score=900, "燐音色纸" score=900
→ 两候选分差<300 → ResolveResult::Ambiguous({candidates})
→ ValidationOutcome::NeedConfirm("'燐音'匹配到多个商品：燐音吧唧、燐音色纸。请回复确认")
→ 不落事件, 等待确认
```

### 5.5 商品未找到
```
LLM识别"不存在商品" → alias_match.resolve_item: candidates为空
→ ResolveResult::NotFound
→ ValidationOutcome::Reject("无法识别商品：不存在商品")
→ BotReply列出当前可排商品
```

### 5.6 无效意图
```
成员发"你好" → LLM识别为ParsedIntent::Unknown
→ ValidationOutcome::Ignore
→ BotReply::Silent(群内不回复)
```

---

## 六、结算 (Settlement)

### 6.1 满减按金额比例分摊(最大余数法)
```
优惠满300减50, 用户A原价200元, B原价100元
→ compute_user_basis: [(A, 20000), (B, 10000)], basis_sum=30000
→ allocate_discount_by_ratio(5000):
  A: numerator=5000*20000=100M, floor=100M/30000=3333, remainder=10000
  B: numerator=5000*10000=50M, floor=50M/30000=1666, remainder=20000
  allocated=3333+1666=4999, leftover=1
  按remainder排序: B(20000) > A(10000) → B得+1=1667
→ A.discount_share=3333(33.33元), B.discount_share=1667(16.67元)
→ 总优惠=3333+1667=5000 ✓ 一分不差
```

### 6.2 购物金等额分摊
```
购物金80元, 用户A,B各100原价 → allocate_discount_equal(8000)
→ per_user=8000/2=4000, remainder=0
→ A.discount_share=4000, B.discount_share=4000
```

### 6.3 优惠叠加
```
两条规则: 满300-50 + 购物金80
→ 按规则顺序依次应用: 先满减分摊, 再购物金分摊
→ A: discount_share = 3333(满减) + 4000(购物金) = 7333
→ final_total = 20000 - 7333 = 12667(126.67元)
→ 若final_total<0 → clamp to 0
```

---

## 七、快照发布 (Snapshot Publishing)

### 7.1 快照生成与发布
```
AllocationSnapshot{round_id, version, item_allocations, user_summaries}
→ SnapshotService.save_and_publish: SnapshotRepo.save_allocation持久化到DB
→ snapshot.to_public(): PublicSnapshot{round_id, title, items:[PublicItemView{boxes:[slots]}], user_bills:[PublicUserBill]}
→ R2Publisher.publish_current: PUT rounds/{round_id}/current.json
→ 前端静态页面读取current.json实时展示排位表
```

### 7.2 Replay Timeline发布
```
ReplayEngine.replay() → Vec<ReplayStep>
→ 每步: ReplayStep{step_index, state_diff:StateDiff{slot_changes, claim_changes, ...}, decision_trace:DecisionTrace}
→ TimelineStore.save_step持久化(每N步存完整快照, 其余存diff)
→ R2: rounds/{round_id}/replays/{replay_id}/steps/000001.json
→ 前端Replay Viewer逐步播放
```

---

## 八、事件回放 (Replay)

### 8.1 从事件重建状态
```
round_id + events[] → ReplayService.rebuild_snapshot
→ sorted_events按effective_at ASC, sequence ASC排序
→ collect_effective_claims: 遍历events, 应用ClaimCreated(新建claim)、ClaimCancelled(3种撤销方式)、ClaimModified(修改数量/策略)
→ 只保留active claims, 计算每行priority_level
→ AllocationEngine.allocate: 按priority DESC排序 → 填槽
→ 若有DiscountRulesSet事件 → SettlementEngine.settle计算金额
→ 返回(AllocationSnapshot, Option<SettlementSnapshot>)
```

### 8.2 Replay Engine逐步回放(带Trace)
```
ReplayEngine.replay(round_config, events, ReplayOptions{include_settlement=true})
→ for each event:
  before_snapshot = state.to_allocation_snapshot()
  decision_trace = apply_event_with_trace:
    ClaimCreated→state.add_claim→PriorityTrace{user_id, priority_level, matched_eligibility_ids}
    ClaimCancelled→state.apply_cancel→用户claim被清空
    DiscountRulesSet→state.discount_rules更新
  after_snapshot = state.to_allocation_snapshot()
  state_diff = StateDiff.from_snapshots(before, after):
    检测slot变化→判定reason: NewClaimFilled/Filled→empty=CancelReleased/连锁AutoMovedForward/TailSegmentCreated/AdminFixed/AdminUnlocked
    判定causality: Direct(直接)/Cascade(连锁)/Recalculation(重算)
    claim_changes: 被撤销claim status变化
    user_total_changes: 用户总金额变化
    item_total_changes: 商品数量变化
  settlement_snapshot = SettlementEngine.settle(after_snapshot)
  step = ReplayStep{step_index, event_id, before_version, after_version, state_diff, allocation_snapshot, settlement_snapshot, decision_trace}
→ 返回ReplayResult{replay_id, final_snapshot, steps: [ReplayStep]}
```

---

## 九、模拟排谷 (Simulation)

### 9.1 JSONL消息队列模拟
```
queue.jsonl文件 → queue_file.read_jsonl_queue_file: 逐行解析QueueMessageRecord{source_sequence, group_id, user_id, nickname, text, timestamp_ms}
→ 按timestamp_ms→source_sequence→message_id排序
→ for each record:
  ParseRequestContext → MessageParser.parse_member_message → ParsedMessage
  EventValidator.validate → match ValidationOutcome:
    Ok(event) → validated_events.push(event)
    Reject/NeedConfirm/Ignore → parse/validation_failures记录
→ SimulationRunner.run: ReplayEngine.replay(round_config, validated_events, replay_options)
→ 生成ReplayResult{steps, final_snapshot}
→ SimulationReport{replay_id, input_message_count, parsed_event_count, applied_event_count, parse_failure_count, validation_failure_count, final_total_amount}
```

### 9.2 解析缓存模式
```
ParserMode::CachedParse{cache_path} → parse_with_mode
→ ParseCache.get(raw): 用input_hash查缓存, 匹配则直接返回ParsedMessage
→ 不调用LLM, 实现确定性重放
→ ParserMode::HybridCachedThenLlm: 先查缓存, miss再调LLM
→ ParserMode::RuleOnly: 纯规则解析(不做LLM)
→ ParserMode::LiveLlm: 实时LLM(不可复现)
```

---

## 十、幂等与并发 (Idempotency & Concurrency)

### 10.1 消息幂等
```
相同group_id+qq_message_id的RawMessage → RawMessageRepo.find_by_message_id
→ 已存在 → 直接返回之前处理结果, 不重复处理
→ 不存在 → 插入raw_messages表, 继续后续流程
```

### 10.2 Round级别事务锁
```
处理round事件时 → repo::postgres::with_round_lock(pool, round_id)
→ SELECT pg_advisory_xact_lock(hash(round_id.0))
→ 同一round串行处理, 不同round并发
→ 防止管理员API和群消息同时修改同一round导致乱序
```

---

## 十一、错误处理 (Error Handling)

### 11.1 解析失败不入事件
```
LLM返回非JSON或confidence<阈值 → 错误不入EventStore
→ 保存到parsed_messages表(status=failed) → 供排错
→ BotReply告知用户失败原因
→ Replay Timeline中该消息标记为ParseFailed(不入分配)
```

### 11.2 R2发布失败不阻塞业务
```
SnapshotService.save_and_publish: SnapshotRepo.save_allocation成功→R2Publisher.publish_current失败
→ 业务event已持久化, 快照已存DB, 仅R2发布失败
→ 后台重试或管理员手动触发重新发布
→ 不可以rollback已写入的业务event
```

### 11.3 金额为负clamp
```
结算后某用户final_total < 0 → clamp to 0
→ 记录SettlementWarning: "User X final total clamped from -500 to 0"
→ 不抛异常, 继续生成SettlementSnapshot
```

---

## 十二、前端数据流 (Frontend Data Flow)

### 12.1 实时排位表
```
前端GET /public/rounds/{id}/current → public_routes.get_current_snapshot
→ SnapshotService.get_latest → SnapshotRepo.get_latest_allocation
→ 返回AllocationSnapshot JSON(含boxes/slots状态, user_summaries)
→ 前端渲染网格视图: 每个cell显示user_id/display_name, slot_policy标记
```

### 12.2 回放播放器
```
前端GET /api/rounds/{id}/replays → 列出可用replay
→ GET .../replays/{replay_id}/steps?from=0&limit=100 → ReplayStepSummary列表(左侧时间轴)
→ 点击某step → GET .../steps/{step_index} → 完整ReplayStep
→ 中间: 按当前allocation_snapshot渲染排位表
→ 右侧: DecisionTrace(原始消息→解析→校验→优先级→排位→结算)
→ 底部: StateDiff高亮变化slot(绿色=新增, 红色=释放, 黄色=前移)
→ 包尾段用边框/颜色区块标记
```

### 12.3 管理员审计面板
```
管理员登录 → AdminSlotView显示完整user_id/nickname/raw_message_id/claim_id
→ 公开页PublicSlotView只显示display_name(脱敏)
→ 点击step → "创建修正事件": 管理员追加ParseOverrideEvent, 生成新replay_id
→ 对比新旧replay: 最终结果差异 + 关键step差异
```

---

## 附录: 数据结构引用索引

| 输入内容 | 触发入口 | 生成事件类型 | 影响引擎 | 输出物 |
|---------|---------|------------|---------|-------|
| 群成员排谷文本 | message_service::process_as_claim | ClaimCreated | AllocationEngine, SettlementEngine | AllocationSnapshot, BotReply |
| 群成员撤销文本 | message_service::process_as_claim | ClaimCancelled | AllocationEngine(replay) | 槽位释放+连锁前移, BotReply |
| 管理员开团命令 | message_service::handle_admin_message | RoundOpened | RoundService | Round记录 |
| 管理员加商品 | message_service::handle_admin_message | - | AdminService | Item记录 |
| 管理员加优先权 | message_service::handle_admin_message | - | AdminService | Eligibility记录 |
| 管理员设优惠 | message_service::handle_admin_message | DiscountRulesSet | SettlementEngine | 用户账单更新 |
| 管理员锁位 | message_service::handle_admin_message | AdminSlotLocked | AllocationEngine | slot锁定 |
| 管理员固定 | message_service::handle_admin_message | AdminAllocationAdjusted | AllocationEngine | slot强制分配 |
| 管理员结团 | message_service::handle_admin_message | RoundClosed | RoundService | status=Closed, 导出 |
| 导出命令 | message_service::handle_admin_message | - | ExportService | CSV文件 |
| JSONL模拟文件 | simulation_runner::run | 多个ClaimCreated | ReplayEngine | SimulationReport |
| 解析缓存文件 | parse_with_mode(CachedParse) | 同上(确定性) | 同上 | 复现结果 |
| 前端请求快照 | api::public_routes | - | SnapshotService | PublicSnapshot JSON |
| 前端请求回放 | api::replay_routes | - | ReplayEngine | ReplayStep数组 |
