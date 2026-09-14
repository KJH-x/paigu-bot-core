# 排谷模拟验证报告（多 Agent 话术生成 + 确定性重放）

> 本文件为脱敏公开版：真实昵称以 `用户A` / `用户B` 代替；内部原版见 `VERIFICATION.md`（gitignored）。

日期：2026-09-14　对象：`paigu-bot-core`（`workspace/paigu-bot-core-20260510`）

## 1. 目标与方法

需求链路：**群内 bot → 获取消息（保存）→ paigu core 处理 → 输出排结果**。
额外 Policy：**时间验证：购物金优先排，非购物金延后排；允许话术：包盒、包尾。**

方法：
1. 派发 4 个子代理，给出模糊指令，在排谷领域内自由模拟多用户消息传递，生成 4 份语料（JSONL + 商品表 + 手推预期）。
2. 新增**确定性规则解析器** `RuleOnly`（不依赖 LLM），新增 `simulate` 验证器与 `serve` 本地聊天服务器。
3. 用真实引擎（AllocationEngine / ReplayEngine）对每份语料逐条重放，输出逐条状态与最终排结果，与子代理手推预期对照。

## 2. 语料

| Agent | 聚焦 | 消息数 | 用户 | group |
|---|---|---|---|---|
| agent-a-normal | 常规手速排谷 | 52 | 10+admin | 10001 |
| agent-b-box-tail | 包盒/包尾/端盒/锁列 | 49 | 8 | 10002 |
| agent-c-cancel-fund | 撤销/改单/购物金/优先权/优惠 | 57 | 8+admin | 10003 |
| agent-d-adversarial | 歧义/异常/对抗 | 68 | 12 | 10004 |

## 3. 执行结果总览

| Agent | 生效 | 拒绝 | 需确认 | 忽略 | 未支持 | 重复 |
|---|---|---|---|---|---|---|
| a-normal | 52 | 0 | 0 | 0 | 0 | 0 |
| b-box-tail | 49 | 0 | 0 | 0 | 0 | 0 |
| c-cancel-fund | 45 | 0 | 0 | 2 | 10 | 0 |
| d-adversarial | 39 | 9 | 0 | 15 | 4 | 1 |

（agent-c/d 的「未支持」= 改单 5 条 + 管理员命令 5/4 条；「拒绝」= 数量 0 / 超 99 / 结团后消息。）

## 4. 关键验证结论

### 4.1 agent-a：手推预期被逐槽完全复现 ✅
购物金用户（用户A/用户B）在**晚于**普通用户发言的情况下仍占据 `燐音吧唧 box1` 前 5 槽，
非购物金用户整体后移；`蓝良吧唧 box3` 为包尾段且**不阻塞**后续散户继续填 `box2`；
`燐音亚克力 box1` 被「包盒」独占整盒；`蓝良立牌` 单领 20/20 占满、3 件进入 waiting。
实际输出与 `scenario.md` 5.1–5.4 完全一致。

### 4.2 新增 Policy 已生效
- **购物金优先排 / 非购物金延后排**：排序键 = `priority_level DESC → effective_at ASC → sequence ASC`。
  通过 `Eligibility{priority_level:10, note:购物金, valid_from}` 注入；支持
  ① 管理员 `/加优先 … 备注=购物金`；② 消息自述「我有购物金 / 购物金排…」。
- **时间验证**：`Eligibility::applies_at` 以 `valid_from` 判定，只有授权/自述时刻之后的 claim 才获得优先。
- **允许话术 包盒**：`包盒 / 整盒 / 包一盒 / 一盒全包 / 全包` → `SlotPolicy::FullBox`（独占整盒）。
- **允许话术 包尾**：`包尾 / 包尾巴 / 包个尾 / 要尾 / 留尾 / 端盒 / 端了` → `SlotPolicy::TailLocked`。

### 4.3 本地聊天链路（smoke test）
`serve` 子命令启动内存模拟服务器，无 PostgreSQL 依赖。
9 条消息实测：后发的购物金用户（用户A）占 `box1 s1,s2`；包盒独占亚克力整盒；包尾锁空；
「你好」→ Ignored；「排燐音吧唧999」→ Rejected。证据见 `chat-smoke-test/`。

## 5. 发现并已修复的缺陷

| # | 缺陷 | 证据 | 修复 |
|---|---|---|---|
| D1 | **「包盒」完全未识别**（normalize/classify 只认 包尾/端盒） | agent-b b11/b32、agent-c c22、agent-d d15/d19 | 新增 `SlotPolicy::FullBox` + `allocate_full_box` + 归一化/校验映射 |
| D2 | **购物金优先无数据通道**，优先级恒 0 | agent-a 手推 vs 无 priority | Eligibility 注入 + 自述识别 + 时间验证 |
| D3 | 带商品的撤销被误判为「歧义」→ 全部拒绝 | agent-b b25、agent-c 9 条 `撤X1` 全 Rejected | 歧义判定改为「同时含撤销与显式排谷动词」 |
| D4 | 包盒无显式数量时被中文「一」误解析为 1 | agent-b b29/b30「一盒全包/包一盒」只占 1 槽 | FullBox 数量取 `box_size` |
| D5 | 包尾数量 > 盒规时创建超规盒 | agent-b b23 `罐徽包尾8`(盒规6)、agent-d d18 `包尾20` | `allocate_tail_locked` clamp 到 `box_size` |
| D6 | 数量 0 / 超大（999、100）直接排入，产生上百盒 | agent-d d40/d41/d52 | 校验层：qty=0 与 qty>99 拒绝 |
| D7 | 闲聊/无法识别被判「置信度不足→拒绝」（应静默忽略） | agent-d 15 条闲聊、agent-c c12/c41 | 校验顺序改为 Unknown→Ignore、歧义→NeedConfirm、再置信度 |
| D8 | `AllocationSnapshot.version` 恒为 1 | chat「当前版本 #1」恒定 | 按事件数/状态版本回填 |

## 6. 仍待决 / 未实现（需产品确认）

- **O1 改单**：`ParsedIntent::Modify` 已能解析，但校验层未实现（agent-c 5 条 Unsupported）。
- **O2 管理员事件未进入重放**：`/锁位 /修正 /设置优惠 /加商品` 未写入 ReplayEngine 可消费的事件，
  故锁位/固定/优惠不影响排结果。
- **O3 锁列**：`ColumnLocked` 无专门语义，当前按普通槽处理（agent-b 锁列话术）。
- **O4 包尾语义歧义**：当前实现 = 开新盒占前 N 槽并锁其余；字面「包尾」亦可理解为占末尾/补满当前盒。需确认。
- **O5 赠品**：Gift 商品（特典色纸）被当作拼团排入盒（agent-c 出现 box1–box3）。需确认赠品是否可排。
- **O6 单领单价**：引擎 `allocate_single` 写入 `unit_price=0`，未回填商品单价。
- **O7 歧义触发依赖别名表**：agent-d「排燐音」因别名不重叠而唯一命中，未触发 NeedConfirm；
  要在商品表层面保证重叠别名才能验证歧义路径。
- **O8 结算**：`ReplayResult.final_settlement` 恒 None（每步有 settlement，最终字段未回填）。

## 7. 复现命令

```bash
# 确定性重放验证（逐条状态 + 最终排结果 + 报告）
cargo run -- simulate \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --queue        simulation-corpus/agent-a-normal/queue.jsonl \
  --out          simulation-corpus/agent-a-normal/out
# 输出: out/report.md, out/result.json, out/outcomes.jsonl

# 本地聊天界面模拟（无需 PostgreSQL）
cargo run -- serve \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --port 8090
# 浏览器打开 http://127.0.0.1:8090
```

## 8. 新增/改动代码

- 新增 `src/parser/rule_parser.rs`：确定性中文话术解析（商品/别名/数量/包盒/包尾/单领/代牌/撤销/歧义）。
- 新增 `src/simulation/verifier.rs`：语料加载、逐条解析校验、重放、报告输出。
- 新增 `src/simulation/chat_server.rs`：本地聊天服务器 + 内存会话 + 实时排结果。
- 改 `src/domain/claim.rs`（`SlotPolicy::FullBox`）、`src/parser/normalize.rs`、`src/parser/validation.rs`、
  `src/engine/allocation_engine.rs`（FullBox/包尾 clamp）、`src/replay/replay_engine.rs`（version）、
  `src/engine/replay.rs`（version）、`src/main.rs`（`simulate` / `serve` 子命令）。
- 既有 9 个测试全部通过。
