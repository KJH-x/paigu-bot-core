# agent-d-adversarial — 对抗/模糊/异常语料场景说明

- round_id: `es_2026_05`（ES5月新谷），group_id: `10004`
- 语料：`queue.jsonl`（68 行，12 用户，message_id 前缀 `d`，attachments=[]，reply_to=null）
- 配置：`round_config.json`（4 商品，名称/别名严格按题面）
- 视角：对抗 / 模糊 / 异常 / 干扰，用于验证解析与校验的健壮性
- 校验规则回顾（`src/parser/validation.rs`）：
  1. `confidence < threshold` → **Reject**（"识别置信度不足…请按格式重发"）
  2. `ambiguous_parts` 非空 → **NeedConfirm**（"以下部分不确定…请回复确认"）
  3. `Claim`：逐商品 `alias_match::resolve_item`，未命中 → **Reject**（"无法识别商品：X" 或歧义提示）
  4. `AdminCommand` 意图 → **Reject**（"管理员命令请使用斜杠命令格式"）
  5. 其余意图（Unknown/Modify/ConfirmAmbiguous 等）→ **Ignore（不回复）**

> 说明：本语料以"parser 会填 `ambiguous_parts`"为前提推演，并同时给出**纯 `alias_match` 规则**下的推演，二者不一致处即为存疑点（见文末）。

---

## 一、逐条预期系统行为

### A. 正常消息（应成功，保证有可验证结果）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 1 | d1 | u_a 甲 | 排燐音吧唧2 | **成功** badge_rinne ×2 |
| 2 | d2 | u_b 乙 | 排蓝良吧唧1 | **成功** badge_ran ×1 |
| 3 | d3 | u_c 丙 | 蓝良立牌单领1 | **成功** stand_ran ×1（single） |
| 4 | d4 | u_d 丁 | 排燐音亚克力3 | **成功** acrylic_rinne ×3 |
| 5 | d5 | u_e 戊 | 排燐音吧唧1 蓝良吧唧1 | **成功** badge_rinne ×1 + badge_ran ×1（一条消息两 line） |
| 7 | d7 | u_g 庚 | 排亚克力1 | **成功** acrylic_rinne ×1（别名 亚克力） |
| 8 | d8 | u_h 辛 | 立牌2 | **成功** stand_ran ×2 |
| 9 | d9 | u_i 壬 | 排蓝良2 | **成功** badge_ran ×2（别名 蓝良，规则下唯一） |
| 10 | d10 | u_j 癸 | 排燐音吧唧3 | **成功** badge_rinne ×3 |
| 11 | d11 | u_k 子 | 排蓝良吧唧2 | **成功** badge_ran ×2 |
| 65 | d65 | u_c 丙 | 排燐音亚克力2 | **成功** acrylic_rinne ×2 |
| 67 | d67 | u_g 庚 | 排蓝良吧唧1 | **成功** badge_ran ×1 |
| 68 | d68 | u_h 辛 | 排燐音吧唧1 | **成功** badge_rinne ×1 |

### B. 购物金优先 Policy（Policy 1）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 6 | d6 | u_f 己 | 购物金排燐音吧唧2 | **成功**；若 u_f 命中购物金 eligibility，priority_level>0，**优先于**更早的 u_a(d1) 排 |
| 12 | d12 | u_b 乙 | 非购物金排燐音吧唧1 | **成功**；非购物金 priority=0，**延后**于购物金用户 |
| 13 | d13 | u_f 己 | 我用购物金优先排蓝良吧唧1 | **成功**；同一购物金用户的第二件 |
| 66 | d66 | u_f 己 | 排燐音吧唧1 | **成功**；购物金用户后续请求仍享优先 |

预期排序（同商品）：`priority_level` 倒序 → 时间正序 → sequence 正序。
即 badge_rinne 上 u_f 的 3 条（d6×2、d66×1）应整体排在所有非购物金请求之前（见第三节手推结果）。

### C. 包盒 / 包尾 Policy（Policy 2，允许话术）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 14 | d14 | u_c 丙 | 包尾燐音吧唧2 | **成功**，slot_policy=TailLocked（normalize 命中"包尾"），单独段，不回填空位 |
| 15 | d15 | u_d 丁 | 包盒燐音吧唧 | **成功（存疑）**：normalize 只识别 包尾/端盒，**未识别"包盒"**，可能退化为普通 qty=10 或数量缺失=1 |
| 16 | d16 | u_e 戊 | 包尾蓝良吧唧8 | **成功**，TailLocked ×8（< box_size 10，合法） |
| 17 | d17 | u_k 子 | 包盒燐音吧唧 蓝良立牌单领1 | **边界**：包盒(TailLocked?) + 单领(single) 同条；两 line 分别校验/分配 |
| 18 | d18 | u_g 庚 | 包尾燐音吧唧20 | **边界/应拒绝或告警**：20 > box_size 10，超盒规 |
| 19 | d19 | u_h 辛 | 包盒蓝良吧唧 | **成功（存疑）**，同 d15 |
| 20 | d20 | u_i 壬 | 包尾亚克力9 | **边界**：9 > box_size 8，超盒规 |

### D. 歧义（应需确认 NeedConfirm）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 21 | d21 | u_j 癸 | 排燐音 | **需确认**（"燐音"同时命中 燐音吧唧/燐音亚克力） |
| 22 | d22 | u_a 甲 | 排燐音1 | **需确认** |
| 23 | d23 | u_c 丙 | 我要燐音 | **需确认** |
| 24 | d24 | u_d 丁 | 排吧唧 | **需确认**（"吧唧"同时命中 燐音吧唧/蓝良吧唧） |
| 25 | d25 | u_e 戊 | 排立牌 | **成功**：别名"立牌"精确命中 stand_ran（1200 分，唯一），不歧义 |

> 规则层备注：纯 `alias_match` 下 `燐音` 对 badge_rinne=1200、acrylic_rinne=400，差值 800≥300 → **会判为 resolved**，并不会歧义。需依赖 parser 填 `ambiguous_parts` 才能得到 NeedConfirm。此为核心存疑点。

### E. 商品未找到（应拒绝 Reject）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 26 | d26 | u_k 子 | 排xxx | **拒绝** "无法识别商品：xxx" |
| 27 | d27 | u_h 辛 | 排初音 | **拒绝** 商品不存在 |
| 28 | d28 | u_i 壬 | 排手办 | **拒绝** 商品不存在 |
| 29 | d29 | u_j 癸 | 排骨 | **拒绝**（"排谷"错写成"排骨"，无匹配商品） |

### F. 无效 / 闲聊（应忽略 Ignore，不回复）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 30 | d30 | u_a | 你好 | **忽略** |
| 31 | d31 | u_b | 在吗 | **忽略** |
| 32 | d32 | u_c | [图片] | **忽略**（attachments 为空） |
| 33 | d33 | u_d | 图 | **忽略** |
| 34 | d34 | u_e | 😂 | **忽略** |
| 35 | d35 | u_f | ！！！！！ | **忽略**（纯符号） |
| 36 | d36 | u_g | @甲 你排了吗 | **忽略**（含"排"但无商品，属聊天） |
| 37 | d37 | u_h | ""（空） | **忽略** |
| 38 | d38 | u_i | 哈哈哈哈哈哈 | **忽略** |
| 39 | d39 | u_j | 某成员 | **忽略** |

### G. 异常格式

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 40 | d40 | u_k | 排燐音吧唧0 | **边界**：qty=0 → 事件成立但 `Claim::is_empty`，**无槽位**；应拒绝或静默 |
| 41 | d41 | u_a | 排燐音吧唧999 | **边界**：qty=999，max_quantity=null → 规则不拦；分配会溢出多盒，应告警 |
| 42 | d42 | u_b | 排燐音吧唧 | **成功**：数量缺失，parser 默认 qty=1（预期） |
| 43 | d5(重复) | u_c | 排蓝良吧唧1 | **异常**：message_id 与 seq5 的 d5 重复；intake 生成新 raw_message_id，但 qq_message_id 冲突，去重行为未定义 |
| 44 | d44 | u_d | 排燐音吧 | **存疑**：截断文本；规则下 `"燐音吧唧".contains("燐音吧")` → +400，唯一命中 badge_rinne，可能被错误接受 |
| 45 | d45 | u_e | 排rinne badge 2 | **存疑**：中英混杂，parser 可能切出 "rinne badge" 导致匹配失败/低置信 → 拒绝 |
| 46 | d46 | u_f | claim 燐音吧唧 x2 | **成功**：中英混杂但商品名完整，qty=2 |
| 47 | d47 | u_g | 牌燐音吧唧2 | **忽略**：错别字"排→牌"，`classify_message` 不命中 排/单领/claim/包尾/端盒 → Unknown → Ignore（用户得不到任何反馈） |
| 48 | d48 | u_h | 排骨燐音吧唧1 | **存疑**：含"排"，parser 名称含"排骨"；规则下 parsed.name.contains(item.name) → badge_rinne +350，可能误接受 |
| 49 | d49 | u_i | 排 燐音 吧 唧 1 | **成功**：空格分隔，parser 归一化后应为 燐音吧唧 ×1 |
| 50 | d50 | u_j | 排燐音吧唧２ | **存疑**：全角数字，qty 解析结果不确定（2 或失败） |
| 51 | d51 | u_k | 排燐音吧唧二 | **存疑**：中文数字"二"，qty 解析结果不确定 |
| 52 | d52 | u_a | 排蓝良吧唧一百 | **边界**：中文数字"一百"→ qty=100，异常巨大，应拒绝或告警 |
| 63 | d63 | u_d | 超长多意图串 | **存疑**：超长 + 多商品 + 撤销 + 包盒/包尾混排；易触发解析截断/低置信 → 整体拒绝或部分解析 |
| 64 | d64 | u_e | 排蓝良吧唧1（附言：…） | **成功（预期）**：应只取首句有效 claim；若 parser 受附言干扰则降置信 |

### H. 干扰（并发 / 连续快速）

| seq | mid | 用户 | text | 预期 |
|----|-----|------|------|------|
| 52/53 | d52/d53 | u_a / u_b | 同一 timestamp 1778241610180 | **并发**：queue_file 按 ts→sequence→message_id 排序，d52 先于 d53；同商品则 a 占先 |
| 54 | d54 | u_c | 排燐音吧唧1 | 与 d53 相差 180ms，正常竞争 |
| 55 | d55 | u_b | 排蓝良吧唧1 | **乱序**：ts=1778241609950 < d54(…0360)，排序后被提前到 d54 之前 |
| 56 | d56 | u_b | 排亚克力1 | 同一用户 u_b 连续快速排（d53/d55/d56/d57） |
| 57 | d57 | u_b | 排立牌1 | 同一用户快速连续 |

### I. 管理员命令边界（非管理员 is_admin=false）

| seq | mid | 用户 | text | is_admin | 预期 |
|----|-----|------|------|----------|------|
| 58 | d58 | u_admin 群主 | /锁位 燐音吧唧 box1 | true | **管理员路径**（AdminLockSlot） |
| 59 | d59 | u_admin 群主 | /结团 | true | **管理员路径**（AdminCloseRound） |
| 60 | d60 | u_k 子 | /结团 | **false** | **忽略**：`classify_message` 对非管理员 `/` 前缀返回 Unknown → Ignore（不回复）；若 parser 直接给 AdminCommand 意图则会 **Reject** |
| 61 | d61 | u_k 子 | /加商品 测试 100 5 | **false** | **忽略**（同上） |
| 62 | d62 | u_a 甲 | /开团 测试团 | **false** | **忽略**（同上） |

---

## 二、异常类型分布（68 行）

| 类别 | 条数 | seq |
|------|------|-----|
| 正常 claim（含别名/多商品/单领） | 13 | 1–5,7–11,65,67,68 |
| 购物金优先 | 4 | 6,12,13,66 |
| 包盒/包尾 | 7 | 14–20 |
| 歧义 | 4 | 21–24 |
| 未找到 | 4 | 26–29 |
| 无效/闲聊/纯符号/@/空/超短 | 10 | 30–39 |
| 数量异常（0/999/缺失/全角/中文数/百） | 6 | 40,41,42,50,51,52 |
| 重复 message_id | 1 | 43 |
| 截断/混杂/错别字/超长/附言 | 8 | 44,45,46,47,48,49,63,64 |
| 干扰（并发/乱序/连续） | 6 | 52,53,54,55,56,57 |
| 管理员/非管理员命令 | 5 | 58–62 |

预期结果汇总（手推）：**成功 ≈ 30，需确认 4，拒绝 ≈ 8，忽略 ≈ 18**（其余为边界成功/存疑）。

---

## 三、手推最终排结果（预期）

前提：parser 正常识别，u_f 命中购物金 eligibility（priority>0），非购物金 priority=0；分配按 `priority_level↓ → effective_at↑ → sequence↑`，普通槽位填满再开新盒；TailLocked 独立成段。

### 1) 燐音吧唧 badge_rinne（box_size=10，split）

有效请求（Normal，去重后）：
- 购物金：u_f d6×2、d66×1
- 非购物金：u_a d1×2、u_e d5×1、u_j d10×3、u_b d12×1、d53×1、u_c d54×1、u_h d68×1、（存疑 d44 u_d×1、d48 u_h×1、d49 u_i×1、d50 u_j×2、d51 u_k×2）

预期普通段填充顺序（前 10 格 = box1）：
`u_f,u_f,u_f`（购物金优先）→ `u_a,u_a,u_e,u_j,u_j,u_j,u_b` → box1 满
box2：`u_b(d53),u_c(d54),u_h(d68),…`（存疑项随后）
TailLocked：u_c d14 ×2 → 独立段（新盒 slot1–2，slot3–10 锁空）；u_g d18 ×20 超盒规 → **应拒绝/告警**，不入段。

### 2) 蓝良吧唧 badge_ran（box_size=10）

有效：u_f d13×1（购物金优先）、u_b d2×1、u_e d5×1、u_i d9×2、u_k d11×2、u_b d55×1、u_g d67×1
预期 box1：`u_f, u_b, u_e, u_i, u_i, u_k, u_k, u_b, u_g` → 9/10
TailLocked：u_e d16 ×8 → 独立段。

### 3) 燐音亚克力 acrylic_rinne（box_size=8）

有效：u_d d4×3、u_g d7×1、u_b d56×1、u_c d65×2
预期 box1：`u_d×3, u_g, u_b, u_c×2` → 7/8
TailLocked：u_i d20 ×9 > 8 → **应拒绝/告警**。

### 4) 蓝良立牌 stand_ran（single，max_quantity=5）

有效：u_c d3×1、u_h d8×2、u_b d57×1 → 已占 4；u_k d17 若解析出 ×1 → 5/5（满）；u_e d25×1 → 超出 → 进 waiting。

> 以上为**语义层手推**；实际槽位序号还取决于 parser 对存疑项的最终判定与购物金 eligibility 表内容，需以 replay 输出为准。

---

## 四、存疑点（需人工/规则确认）

1. **"排燐音"歧义性**：题面要求判歧义，但 `alias_match::resolve_item` 下 badge_rinne=1200 vs acrylic_rinne=400，差 800≥300 → 规则会**直接 resolved 到 燐音吧唧**。只有 parser 主动填 `ambiguous_parts` 才 NeedConfirm。**规则与预期不一致**，是本语料最重要的对抗发现。
2. **"包盒"未纳入话术**：`normalize_claim_item` 只识别 `tail/包尾/端盒`，`classify_message` 只识别 `包尾/端盒`；Policy 要求"允许包盒"，但代码无"包盒"分支 → 包盒语义（整盒=box_size）可能丢失。
3. **购物金优先无数据通道**：`queue.jsonl` / `round_config.json` 均无 eligibility/购物金字段；policy 1 无法端到端验证，需扩展配置（如 `eligibility.json` 或在 round_config 增加 `eligibility` 数组）。
4. **qty=0**：`validate` 不拦，生成 `ClaimLine{quantity:0}`，`Claim::is_empty()` 为真但事件仍 Ok；应拒绝或静默。
5. **重复 message_id**：`intake::to_raw_message_record` 每次新建 `raw_message_id`(uuid)，但 `qq_message_id` 重复，去重/幂等策略未定义。
6. **非管理员斜杠命令**：`classify_message` 返回 Unknown→Ignore，但若 parser 输出 `AdminCommand` 意图，`validate` 返回 Reject("管理员命令请使用斜杠命令格式")；两条路径结论不同。
7. **错别字静默**："牌燐音吧唧"不命中关键词 → Ignore，用户无任何反馈（体验问题）。
8. **截断文本被接受**："排燐音吧"经 `contains` +400 命中 badge_rinne，可能误接受。
9. **包尾超盒规无钳制**：`allocate_tail_locked` 对 `i in 0..quantity` 直接 `fill_slot`，未按 `box_size` 截断；qty>box_size 会生成超规格盒，仅 `(quantity+1)..=box_size` 的锁空段逻辑在越界时为空操作。
10. **超长消息**：d63 多意图长串在 RuleOnly/无缓存模拟下可能直接 `ParseError::CacheMiss`；LiveLlm 下可能截断或降置信。
11. **乱序时间戳语义**：queue_file 以 `timestamp_ms` 为主排序键，迟发但时间戳更早的消息会被前移，符合"手速团按时间戳"设计，但可能与实际到达顺序不符。
