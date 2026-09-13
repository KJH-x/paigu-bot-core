# agent-a-normal — 常规手速排谷场景

## 1. 场景概述

- round_id = `es_2026_05`，title = `ES5月新谷`，group_id = `10001`。
- 52 条群消息，时间跨度 1250ms（`1778241601000` → `1778241602250`），大量消息落在 1~90ms 的并发窗口内，模拟「开团后手速爆发」。
- 10 个成员用户 + 2 条由团长（`u_tsukasa`）发出、`is_admin=true` 的 `/加优先` 授权消息（用于把「购物金」优先权写进事件流，使语料自洽、可离线重放）。
- 覆盖话术：`排X2` / `要X两个` / `X+1` / `帮排X1` / `X 3个谢谢` / 中文数字（两个、三个）/ 别名（rinne、亚克力、立牌）/ 错位语序（`吧唧燐音1`）/ 多商品一条（`排燐音吧唧1 蓝良吧唧1`）/ @某人 / 语气词（捏、蹲）/ 代牌 / 包盒 / 包尾 / 重复发送。

## 2. 商品表（与 round_config.json 一致）

| item_id | name | kind | unit_price_cents | box_size | max_quantity | aliases |
|---|---|---|---|---|---|---|
| badge_rinne | 燐音吧唧 | split | 4500 | 10 | null | 燐音, rinne, 吧唧 |
| badge_ran | 蓝良吧唧 | split | 4500 | 10 | null | 蓝良, ran, 蓝良吧唧 |
| stand_ran | 蓝良立牌 | single | 6000 | null | 20 | 蓝良立牌, 立牌 |
| acrylic_rinne | 燐音亚克力 | split | 3800 | 8 | null | 亚克力, 燐音亚克力 |

## 3. 用户清单

| user_id | nickname | 购物金 | priority_level | 说明 |
|---|---|---|---|---|
| u_rinne | 凛音 | 否 | 0 | 手速主力，燐音/蓝良/亚克力/立牌都排 |
| u_ran | 蓝良推 | 否 | 0 | 蓝良向，含重复发送 |
| u_tsukasa | 司 | 否 | 0 | **团长（admin）**，发 `/加优先`；同时参与排谷，含包盒/包尾 |
| u_hiyori | 日和 | 否 | 0 | 多商品连发 |
| u_jun | 润 | 否 | 0 | 含代牌 |
| u_sora | 空 | 否 | 0 | 含 @ 提及 |
| u_niki | 仁兔 | 否 | 0 | 语气词话术 |
| u_aoi | 葵 | 否 | 0 | — |
| u_kanata | 奏汰 | **是** | 10 | 范围=全部商品（a1 授权） |
| u_mayoi | 真宵 | **是** | 10 | 范围=全部商品（a2 授权） |

> 购物金 = `Eligibility{priority_level:10, scope:全部, note:"购物金"}`，由 a1/a2 两条 admin 消息建立。

## 4. 排序规则（Policy）

同一商品内，有效行按 **`priority_level DESC` → `effective_at ASC` → `sequence ASC` → `line_index ASC`** 排序后再分配。
即：**购物金优先 > 生效时间（消息时间戳）> 消息序号**。
拼团槽位从最小盒号、最小槽号依次填充；一盒填满后开下一盒。

## 5. 预期排结果

### 5.1 燐音吧唧 badge_rinne（box_size=10）

分配顺序（购物金先）：kanata(2) → mayoi(1) → mayoi(1) → kanata(1) → 其余按时间。

- box1：s1=奏汰, s2=奏汰, s3=真宵, s4=真宵, s5=奏汰, s6=凛音, s7=凛音, s8=司, s9=日和, s10=日和
- box2：s1=日和, s2=润, s3=润, s4=空, s5=葵, s6=仁兔, s7=仁兔, s8=凛音, s9=润, s10=凛音
- box3：s1=日和, s2=仁兔, s3=葵, s4~s10=空

关键点：奏汰/真宵各条消息时间（450/520/1020/1150ms）**晚于**凛音(100ms)、日和(230ms) 等，但因购物金 priority=10，仍占据 box1 前 5 槽；非购物金用户整体后移。

### 5.2 蓝良吧唧 badge_ran（box_size=10）

分配顺序：mayoi(2, 购物金) → ran(1) → aoi(2) → niki(1) → sora(2) → jun(1) → rinne(1) → hiyori(2) → tsukasa(1) → **tsukasa 包尾(1)** → rinne(1) → ran(1) → sora(1) → hiyori(1)

- box1：s1=真宵, s2=真宵, s3=蓝良推, s4=葵, s5=葵, s6=仁兔, s7=空, s8=空, s9=润, s10=凛音
- box2：s1=日和, s2=日和, s3=司, s4=凛音, s5=蓝良推, s6=空, s7=日和, s8~s10=空
- box3（包尾段，TailLocked）：s1=司(Filled)，s2~s10=LockedEmpty（segment=`tail:u_tsukasa:...`）

关键点：包尾段 **不阻塞** 后续普通散户——a41/a44/a45/a47 的普通请求（晚于包尾消息）继续填入 box2 的 s4~s7，而非进入 box3。

### 5.3 燐音亚克力 acrylic_rinne（box_size=8）

- box1：s1~s8 全部=司（a5 `燐音亚克力包盒`，占满整盒）
- box2：s1=润, s2=空, s3=空, s4=空, s5=凛音, s6=葵, s7=日和, s8=日和
- box3：s1=葵, s2=空, s3=润, s4~s8=空

关键点：包盒在最早时间(150ms)发出，独占 box1；无论解析为「数量=8 普通」还是「TailLocked 数量=8」，结果一致（qty == box_size，无剩余槽需锁）。

### 5.4 蓝良立牌 stand_ran（single, max_quantity=20）

排序（购物金先）：mayoi(4) → kanata(2) → ran(1) → hiyori(2) → sora(1) → aoi(1) → jun(3) → niki(2) → rinne(2) → tsukasa(3→截2) → niki(1) → ran(1)

**单领计数（共 20/20 占满）**：

| 用户 | 数量 |
|---|---|
| 真宵 | 4 |
| 奏汰 | 2 |
| 蓝良推 | 1 |
| 日和 | 2 |
| 空 | 1 |
| 葵 | 1 |
| 润 | 3 |
| 仁兔 | 2 |
| 凛音 | 2 |
| 司 | 2（请求 3，仅 2 入账） |

**延后/等待（waiting）**：司 +1、仁兔 +1、蓝良推 +1（共 3 件因达到上限 20 进入 waiting）。
注意：因购物金优先，晚发的真宵(690ms)/奏汰(790ms) 挤入前位，导致末尾的仁兔(920ms)、蓝良推(1200ms) 被截到 waiting。

## 6. 假设与存疑点

1. **购物金的可复现方式**：本语料用 a1/a2 两条 `is_admin=true` 的 `/加优先 … 备注=购物金` 消息建立 `Eligibility`，使「购物金优先」可从语料离线重放得到；其余 50 条 text 均为中文口语。若校验要求「全部 text 为纯口语」，需将这两条改为前置 fixture（`Eligibility` 预置），此时预期结果不变。
2. **购物金作用域**：假设范围=全部 4 个商品（含单领）。若实际只作用于 split 商品，则 5.4 的单领顺序与 waiting 名单会变化。
3. **`包盒` 识别缺口**：`normalize.rs` 只把 `tail/包尾/端盒` 映射为 `TailLocked`，`command_router.rs` 也只匹配 `包尾/端盒`，**均未识别「包盒」**。本语料中因包盒数量恰等于 box_size，两种解析结果相同；但一般情况（包盒数量 ≠ box_size，或需锁整盒）行为未定义，属待确认点。
4. **包尾语义**：`allocation_engine.rs::allocate_tail_locked` 实际是「开一个新盒 + 填前 q 槽 + 锁其余槽」，与 `ARCHITECTURE.md` 中「从当前最后一盒某位置开始锁定」的描述不一致。本预期结果按**代码实现**推导。
5. **单领单价**：引擎中 `allocate_single` 写入 `unit_price = MoneyCents::zero()`，与商品表 6000 分不一致，疑似实现细节/待修，不影响槽位与计数。
6. **重复发送**：a44（蓝良推重复「蓝良吧唧1」）被当作**新增一条 claim**（+1），未做同用户同商品去重；若引擎有去重策略，5.2 结果需相应减少。
7. **代牌**：a30 `燐音吧唧代牌1` 仅置 `is_proxy_card=true`，排队逻辑不变（按普通 +1 计入 5.1）。
8. **`吧唧` 别名歧义**：`badge_rinne.aliases` 含 `吧唧`，单独出现「吧唧」会偏向燐音吧唧；本语料所有消息均带区分词（燐音/蓝良/rinne），未触发歧义。
9. **跨商品排序**：引擎对全部 claim line 做全局排序但按商品独立分配状态，故跨商品顺序不影响结果，仅同商品内相对顺序有意义。
