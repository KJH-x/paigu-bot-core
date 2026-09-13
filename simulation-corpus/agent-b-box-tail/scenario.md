# agent-b-box-tail — 包盒 / 包尾 / 端盒 / 锁列 语料场景

## 1. 场景说明

- round_id: `es_2026_05`，title: `ES5月新谷`，group_id: `10002`。
- 49 条群消息（`queue.jsonl`，message_id 前缀 `b1..b49`），时间基准 `1778241601000`，
  步进约 500ms；`b1/b2`、`b4/b5`、`b7/b8` 为同毫秒并发（同 ts 时按 `source_sequence` 决胜）。
- 全部消息 `attachments=[]`、`reply_to_message_id=null`、`is_admin=false`。
- 目标：覆盖包盒/整盒/一盒全包/包一盒、包尾/包尾巴/包个尾/要尾/留尾、端盒/端了、
  锁列/锁一整列，以及「时间验证 / 购物金优先」和包尾/包盒的各类边界。

商品表见 `round_config.json`：

| item_id | name | kind | unit_price | box_size | max_quantity | aliases |
|---|---|---|---|---|---|---|
| badge_rinne | 燐音吧唧 | split | 4500 | 10 | - | 燐音, rinne, 吧唧 |
| badge_ran | 蓝良吧唧 | split | 4500 | 10 | - | 蓝良, ran |
| can_badge | 燐音罐徽 | split | 3000 | 6 | - | 罐徽, 罐子, 燐音罐徽 |
| stand_ran | 蓝良立牌 | single | 6000 | - | 20 | 立牌, 蓝良立牌 |

## 2. 用户清单

| user_id | nickname | 购物金 | 备注 |
|---|---|---|---|
| u_a | 阿甲 | ✅（priority_level 建议 10） | 多条、含包一盒 |
| u_e | 戊戊 | ✅（priority_level 建议 10） | 三条 split 均后发先排 |
| u_b | 小乙 | ❌ | 包尾巴后撤销 |
| u_c | 丙丙 | ❌ | 包尾、包尾超量 |
| u_d | 丁丁 | ❌ | 整盒、留尾 |
| u_f | 阿己 | ❌ | 锁列、一盒全包 |
| u_g | 庚庚 | ❌ | 端盒、锁一整列、包盒 |
| u_h | 辛辛 | ❌ | 端了、包个尾、包尾 |

> 购物金标记 **只存在于本说明**：`queue.jsonl` 无对应字段，引擎需从用户画像/资格事件
> （`Eligibility.priority_type`）取优先级。见存疑点 #5。

## 3. 预期排结果（手推）

约定：`boxN/sM` = 第 N 盒第 M 槽；`[tail]` = TailLocked 段占用槽；`[lock]` = 该段锁定空槽。
本推演采用仓库现状语义（`ARCHITECTURE.md` 三十一 + `allocate_tail_locked`）：

- **包尾（TailLocked）**：新开一盒，占 `s1..sN`，并锁 `s(N+1)..box_size`；不回填旧空位，也不阻塞旧盒空位被后到散户填充。
- **包盒/整盒/一盒全包/包一盒**：独占一个新盒的全部 `box_size` 槽。
- **锁列（ColumnLocked）**：引擎当前无实现，按 `allocate_normal` 处理（见存疑点 #2）。

### 3.1 badge_rinne 燐音吧唧（box_size 10）

分配顺序（先按 priority_level 降序，再 ts/seq）：b5(u_a,3) → b8(u_e,2) → b31(u_e,包盒) → b38(u_e,1)
→ b1(u_b,1) → b11(u_d,整盒) → b13(u_f,1) → b16(u_g,1) → b18(u_h,1) → b37(u_c,2) → b40(u_b,1) → b43(u_f,锁列1)。

| 盒 | s1 | s2 | s3 | s4 | s5 | s6 | s7 | s8 | s9 | s10 |
|---|---|---|---|---|---|---|---|---|---|---|
| box1 | u_a | u_a | u_a | u_e | u_e | u_e | u_b | u_f | u_g | u_h |
| box2 | u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e |
| box3 | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d |
| box4 | u_c | u_c | u_b | u_f | 空 | 空 | 空 | 空 | 空 | 空 |

- box1 满盒；box2 = b31 包一盒（u_e）；box3 = b11 整盒（u_d）；box4 剩余空。
- 关键：u_a/u_e 为购物金，晚发（b5 ts 602000、b8 ts 603000）却排在最早发帖的 u_b(b1)/u_c(b2) 之前。
- 关键：b31 包一盒新开 box2，**不**去补 box1 的空槽；box1 空槽仍由后到的 b13/b16/b18 散户顺序填满。

### 3.2 badge_ran 蓝良吧唧（box_size 10）

分配顺序：b20(u_e,2) → b30(u_a,包一盒) → b41(u_a,1) → b4(u_d,3) → b22(u_g,端盒2) → b24(u_h,包个尾1)
→ b26(u_b,要尾2) → b27(u_f,锁列1) → b28(u_g,锁一整列1) → b29(u_f,一盒全包) → b33(u_b,1)
→ b34(u_d,2) → b36(u_h,1) → b46(u_g,2)。

| 盒 | 内容 |
|---|---|
| box1 | s1 u_e, s2 u_e, s3 u_a, s4 u_d, s5 u_d, s6 u_d, s7 u_f, s8 u_g, s9 u_b, s10 u_d（满盒） |
| box2 | s1..s10 = u_a（b30 包一盒） |
| box3 | s1 u_g, s2 u_g `[tail 端盒2]`；s3..s10 `[lock u_g]` |
| box4 | s1 u_h `[tail 包个尾1]`；s2..s10 `[lock u_h]` |
| box5 | s1 u_b, s2 u_b `[tail 要尾2]`；s3..s10 `[lock u_b]` |
| box6 | s1..s10 = u_f（b29 一盒全包） |
| box7 | s1 u_d, s2 u_h, s3 u_g, s4 u_g, s5..s10 空 |

- b27「锁列1」、b28「锁一整列1」按现状被当作普通排，落在 box1 s7/s8（见存疑点 #2）。

### 3.3 can_badge 燐音罐徽（box_size 6）

`b15 包尾巴1` 被 `b25 撤销罐徽包尾巴1` 撤销 → 不产生分配；`b23 包尾8 > box_size 6` → 拒绝。
有效顺序：b47(u_e,2) → b3(u_f,1) → b7(u_c,包尾2) → b10(u_g,2) → b12(u_h,1) → b17(u_d,留尾1)
→ b21(u_h,端了3) → b32(u_g,包盒) → b39(u_g,1) → b42(u_d,1) → b45(u_c,1) → b49(u_h,包尾1)。

| 盒 | 内容 |
|---|---|
| box1 | s1 u_e, s2 u_e, s3 u_f, s4 u_g, s5 u_g, s6 u_h（满盒） |
| box2 | s1 u_c, s2 u_c `[tail 包尾2]`；s3..s6 `[lock u_c]` |
| box3 | s1 u_d `[tail 留尾1]`；s2..s6 `[lock u_d]` |
| box4 | s1 u_h, s2 u_h, s3 u_h `[tail 端了3]`；s4..s6 `[lock u_h]` |
| box5 | s1..s6 = u_g（b32 包盒） |
| box6 | s1 u_g, s2 u_d, s3 u_c, s4..s6 空 |
| box7 | s1 u_h `[tail 包尾1]`；s2..s6 `[lock u_h]` |

- 边界「包尾不阻塞散户填前面的空盒/空槽」：b7 包尾开 box2 后，b10(u_g,2) 仍落到更早的 box1 s4/s5。
- 边界「两个用户先后包尾同一商品」：u_c(b7)、u_d(b17)、u_h(b21/b49) 各自独立成段、独立盒。
- 边界「包尾后撤销」：b25 撤销 u_b 的包尾巴，重放后 u_b 在罐徽上无分配，其后的盒序号整体前移一位。

### 3.4 stand_ran 蓝良立牌（single，max_quantity 20）

| user | 数量 |
|---|---|
| u_b | 1（b6） |
| u_d | 2（b9） |
| u_a | 4（b14=3 + b48=1） |
| u_f | 2（b19 + b35） |
| u_h | 2（b44） |
| 合计 | 11 / 20（未触顶，无 waiting） |

### 3.5 用户小计（split 槽位 + single 件数）

u_a 18、u_b 6、u_c 5、u_d 19、u_e 17、u_f 16、u_g 15、u_h 10，合计 106。
（split 槽位 = 34(badge_rinne) + 39(badge_ran) + 22(can_badge) = 95；single = 11。）

## 4. 覆盖清单

- 话术变体：包尾(b7)、包尾巴(b15)、包个尾(b24)、要尾(b26)、留尾(b17)、端盒(b22)、端了(b21)、
  包盒(b32)、整盒(b11)、一盒全包(b29)、包一盒(b30/b31)、锁列(b27/b43)、锁一整列(b28)。
- 边界：包尾不阻塞填旧空槽(b10)、多用户先后包尾(b7/b17/b21/b49)、包尾数量>盒规(b23)、
  包盒后普通排开新盒(b31 后 b13/b16/b18)、包尾后撤销(b25)、包盒不补旧盒空槽(b31 vs box1)、
  购物金后发先排(b5/b8/b20/b30/b41/b47 vs 更早的 u_b/u_c/u_d)。
- 混入常规排谷与单领：b1/b2/b4/b13/b16/b18/b33/b34/b36/b37/b40/b46 等，及 3.4 单领。

## 5. 存疑点

1. **包尾槽位语义冲突（最高优先级）**：任务描述「占新盒**末尾**若干槽、锁定剩余槽」，
   而仓库 `allocate_tail_locked` / `ARCHITECTURE.md` 伪码是「占 `s1..sN`、锁 `s(N+1)..box_size`」（正序）。
   本文件的预期结果按**仓库现状（正序）**给出。若采用「末尾」语义，所有 `[tail]/[lock]` 段需整段翻转，
   且「包尾排在序列末尾」的解释也会变化。请先定夺。
2. **锁列 / ColumnLocked 无实现**：`normalize.rs` 仅识别连续子串「锁列」，`validation.rs` 可产出
   `SlotPolicy::ColumnLocked`，但 `allocation_engine::allocate_split_line` 只处理 `TailLocked` 与 `AdminFixed`，
   `ColumnLocked` 落入 `_ => allocate_normal`，即**锁列被当成普通排**。此外「锁一整列」不含连续「锁列」，
   连归一化都命中不到。列的形状（跨盒同列？盒内竖列？）也未定义。
3. **「端了 / 要尾 / 留尾 / 包尾巴 / 包个尾」归一化缺口**：`normalize.rs` 只匹配 `tail / 包尾 / 端盒`；
   上述口语变体若 LLM 未直接输出 `TailLocked`，会被归一化成 `Normal`。
4. **包盒无专用策略**：`SlotPolicy` 无 `FullBox`；「包盒/整盒/一盒全包/包一盒」既未归一化也无引擎分支。
   本文件假定「独占新盒」，但引擎可能把「整盒」解析为 `quantity=box_size` 的 `Normal` 或 `TailLocked`，
   且未定义「已有半盒时包盒是补满旧盒还是新开盒」。建议明确 FullBox 语义与事件表示。
5. **购物金优先的载体**：语料中购物金只体现在文案与用户清单，`queue.jsonl` 无优先级字段。
   引擎须从 `Eligibility`（`priority_type`/`priority_level`）或用户画像取「购物金」优先级；作用域是否限本商品、
   是否与时间戳共同排序（现状：`priority_level` 降序优先于 `effective_at`）需确认。
6. **撤销目标解析**：`ClaimCancelled` 仅带 `target_item_id + quantity`。当同一用户对同一商品有多条
   （如 u_b 在 can_badge 仅 b15 一条，本语料已刻意唯一；但 u_b 在 badge_rinne/badge_ran 有多条）时，
   撤销是 LIFO、FIFO 还是按数量拆解未定义。撤销是否在分配前生效（导致后续盒号前移）也需确认——本文件按「前置生效、整体重排」处理。
7. **包尾数量 > 盒规**：b23（罐徽包尾8，box_size 6）预期「拒绝」。需确认是直接拒绝、截断为 6、
   还是跨盒续包（8 = 6 + 新盒 2）。
8. **并发同 ts 决胜**：b1/b2、b4/b5、b7/b8 同毫秒，`read_jsonl_queue_file` 用 `timestamp_ms → source_sequence → message_id` 排序；
   需确认与网关实际到达顺序一致。
