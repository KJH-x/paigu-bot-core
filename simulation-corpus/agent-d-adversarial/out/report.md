# 模拟验证报告: es_2026_05

- group_id: 10004
- 消息总数: 68
- 生效(Applied): 39
- 拒绝(Rejected): 9
- 需确认(NeedConfirm): 0
- 忽略(Ignored): 15
- 未支持(Unsupported): 4
- 重复(Duplicate): 1

## 购物金优先权 (Eligibility)

- user=u_f level=10 note=购物金(自述) valid_from=2026-05-08T12:00:01.900+00:00

## 逐条消息结果

| seq | msg | user | status | text | detail |
|---|---|---|---|---|---|
| 1 | d1 | u_a | Applied | 排燐音吧唧2 | claim: badge_rinnex2[normal] |
| 2 | d2 | u_b | Applied | 排蓝良吧唧1 | claim: badge_ranx1[normal] |
| 3 | d3 | u_c | Applied | 蓝良立牌单领1 | claim: stand_ranx1[normal] |
| 4 | d4 | u_d | Applied | 排燐音亚克力3 | claim: acrylic_rinnex3[normal] |
| 5 | d5 | u_e | Applied | 排燐音吧唧1 蓝良吧唧1 | claim: badge_rinnex1[normal], badge_ranx1[normal] |
| 6 | d6 | u_f | Applied | 购物金排燐音吧唧2 | claim: badge_rinnex2[normal] |
| 7 | d7 | u_g | Applied | 排亚克力1 | claim: acrylic_rinnex1[normal] |
| 8 | d8 | u_h | Applied | 立牌2 | claim: stand_ranx2[normal] |
| 9 | d9 | u_i | Applied | 排蓝良2 | claim: badge_ranx2[normal] |
| 10 | d10 | u_j | Applied | 排燐音吧唧3 | claim: badge_rinnex3[normal] |
| 11 | d11 | u_k | Applied | 排蓝良吧唧2 | claim: badge_ranx2[normal] |
| 12 | d12 | u_b | Applied | 非购物金排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 13 | d13 | u_f | Applied | 我用购物金优先排蓝良吧唧1 | claim: badge_ranx1[normal] |
| 14 | d14 | u_c | Applied | 包尾燐音吧唧2 | claim: badge_rinnex2[tail_locked] |
| 15 | d15 | u_d | Applied | 包盒燐音吧唧 | claim: badge_rinnex10[full_box] |
| 16 | d16 | u_e | Applied | 包尾蓝良吧唧8 | claim: badge_ranx8[tail_locked] |
| 17 | d17 | u_k | Applied | 包盒燐音吧唧 蓝良立牌单领1 | claim: badge_rinnex10[full_box], stand_ranx1[normal] |
| 18 | d18 | u_g | Applied | 包尾燐音吧唧20 | claim: badge_rinnex20[tail_locked] |
| 19 | d19 | u_h | Applied | 包盒蓝良吧唧 | claim: badge_ranx10[full_box] |
| 20 | d20 | u_i | Applied | 包尾亚克力9 | claim: acrylic_rinnex9[tail_locked] |
| 21 | d21 | u_j | Applied | 排燐音 | claim: badge_rinnex1[normal] |
| 22 | d22 | u_a | Applied | 排燐音1 | claim: badge_rinnex1[normal] |
| 23 | d23 | u_c | Applied | 我要燐音 | claim: badge_rinnex1[normal] |
| 24 | d24 | u_d | Ignored | 排吧唧 | 无法识别为排谷/撤销意图 |
| 25 | d25 | u_e | Applied | 排立牌 | claim: stand_ranx1[normal] |
| 26 | d26 | u_k | Ignored | 排xxx | 无法识别为排谷/撤销意图 |
| 27 | d27 | u_h | Ignored | 排初音 | 无法识别为排谷/撤销意图 |
| 28 | d28 | u_i | Ignored | 排手办 | 无法识别为排谷/撤销意图 |
| 29 | d29 | u_j | Ignored | 排骨 | 无法识别为排谷/撤销意图 |
| 30 | d30 | u_a | Ignored | 你好 | 无法识别为排谷/撤销意图 |
| 31 | d31 | u_b | Ignored | 在吗 | 无法识别为排谷/撤销意图 |
| 32 | d32 | u_c | Ignored | [图片] | 无法识别为排谷/撤销意图 |
| 33 | d33 | u_d | Ignored | 图 | 无法识别为排谷/撤销意图 |
| 34 | d34 | u_e | Ignored | 😂 | 无法识别为排谷/撤销意图 |
| 35 | d35 | u_f | Ignored | ！！！！！ | 无法识别为排谷/撤销意图 |
| 36 | d36 | u_g | Ignored | @甲 你排了吗 | 无法识别为排谷/撤销意图 |
| 37 | d37 | u_h | Ignored |  | 无法识别为排谷/撤销意图 |
| 38 | d38 | u_i | Ignored | 哈哈哈哈哈哈 | 无法识别为排谷/撤销意图 |
| 39 | d39 | u_j | Ignored | 某成员 | 无法识别为排谷/撤销意图 |
| 40 | d40 | u_k | Rejected | 排燐音吧唧0 | 商品 燐音吧唧 数量为 0，未记录。 |
| 41 | d41 | u_a | Rejected | 排燐音吧唧999 | 商品 燐音吧唧 数量 999 超出单次上限 99，请分开发送。 |
| 42 | d42 | u_b | Applied | 排燐音吧唧 | claim: badge_rinnex1[normal] |
| 43 | d5 | u_c | Duplicate | 排蓝良吧唧1 | 重复 message_id，已按幂等忽略 |
| 44 | d44 | u_d | Applied | 排燐音吧 | claim: badge_rinnex1[normal] |
| 45 | d45 | u_e | Applied | 排rinne badge 2 | claim: badge_rinnex2[normal] |
| 46 | d46 | u_f | Applied | claim 燐音吧唧 x2 | claim: badge_rinnex2[normal] |
| 47 | d47 | u_g | Applied | 牌燐音吧唧2 | claim: badge_rinnex2[normal] |
| 48 | d48 | u_h | Applied | 排骨燐音吧唧1 | claim: badge_rinnex1[normal] |
| 49 | d49 | u_i | Applied | 排 燐音 吧 唧 1 | claim: badge_rinnex1[normal] |
| 50 | d50 | u_j | Applied | 排燐音吧唧２ | claim: badge_rinnex2[normal] |
| 55 | d55 | u_b | Applied | 排蓝良吧唧1 | claim: badge_ranx1[normal] |
| 51 | d51 | u_k | Applied | 排燐音吧唧二 | claim: badge_rinnex2[normal] |
| 52 | d52 | u_a | Rejected | 排蓝良吧唧一百 | 商品 蓝良吧唧 数量 100 超出单次上限 99，请分开发送。 |
| 53 | d53 | u_b | Applied | 排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 54 | d54 | u_c | Applied | 排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 60 | d60 | u_k | Unsupported | /结团 | 非管理员发送斜杠命令 |
| 56 | d56 | u_b | Applied | 排亚克力1 | claim: acrylic_rinnex1[normal] |
| 57 | d57 | u_b | Applied | 排立牌1 | claim: stand_ranx1[normal] |
| 58 | d58 | u_admin | Unsupported | /锁位 燐音吧唧 box1 | 管理员锁位/修正暂未进入模拟重放 |
| 59 | d59 | u_admin | Applied | /结团 | 结团 |
| 61 | d61 | u_k | Unsupported | /加商品 测试 100 5 | 非管理员发送斜杠命令 |
| 62 | d62 | u_a | Unsupported | /开团 测试团 | 非管理员发送斜杠命令 |
| 63 | d63 | u_d | Rejected | 排燐音吧唧1，蓝良吧唧1，燐音亚克力1，蓝良立牌单领1，包尾燐音吧唧2，包盒蓝良吧唧，购物金优先排燐音吧唧1，撤销蓝良吧唧1，燐音亚克力2，蓝良2，立牌1，燐音吧唧3，亚克力1，蓝良吧唧2，燐音吧唧4，蓝良立牌2，燐音亚克力3，包尾蓝良吧唧3，包盒燐音吧唧，排燐音吧唧5 这是一条非常长的消息用来测试解析器对超长文本的截断与处理能力同时包含多个商品与多种话术以及撤销意图等复杂语义组合最终观察系统是否能够正确切分并识别其中全部有效商品与意图 | 团已结团，拒绝排谷 |
| 64 | d64 | u_e | Rejected | 排蓝良吧唧1（附言：我昨天说的那个立牌还算数吗，另外帮我看看蓝良吧唧还有没有位置，如果没有就帮我排亚克力吧，谢谢！） | 团已结团，拒绝排谷 |
| 65 | d65 | u_c | Rejected | 排燐音亚克力2 | 团已结团，拒绝排谷 |
| 66 | d66 | u_f | Rejected | 排燐音吧唧1 | 团已结团，拒绝排谷 |
| 67 | d67 | u_g | Rejected | 排蓝良吧唧1 | 团已结团，拒绝排谷 |
| 68 | d68 | u_h | Rejected | 排燐音吧唧1 | 团已结团，拒绝排谷 |

## 最终排结果

### 蓝良吧唧 (badge_ran, split)

- box1: u_f | u_b | u_e | u_i | u_i | u_k | u_k | u_b | · | ·
- box2: u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e | LOCKED | LOCKED
- box3: u_h | u_h | u_h | u_h | u_h | u_h | u_h | u_h | u_h | u_h

### 燐音亚克力 (acrylic_rinne, split)

- box1: u_d | u_d | u_d | u_g | u_b | · | · | ·
- box2: u_i | u_i | u_i | u_i | u_i | u_i | u_i | u_i

### 蓝良立牌 (stand_ran, single)

- 单领: u_cx1, u_hx2, u_kx1, u_ex1
- 等待(waiting): u_bx1

### 燐音吧唧 (badge_rinne, split)

- box1: u_f | u_f | u_f | u_f | u_a | u_a | u_e | u_j | u_j | u_j
- box2: u_b | u_j | u_a | u_c | u_b | u_d | u_e | u_e | u_g | u_g
- box3: u_c | u_c | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED
- box4: u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d
- box5: u_k | u_k | u_k | u_k | u_k | u_k | u_k | u_k | u_k | u_k
- box6: u_g | u_g | u_g | u_g | u_g | u_g | u_g | u_g | u_g | u_g
- box7: u_h | u_i | u_j | u_j | u_k | u_k | u_b | u_c | · | ·

## 结算

- u_g: gross=578.00 discount=0.00 gift=0.00 shipping=0.00 final=578.00
- u_k: gross=690.00 discount=0.00 gift=0.00 shipping=0.00 final=690.00
- u_f: gross=225.00 discount=0.00 gift=0.00 shipping=0.00 final=225.00
- u_b: gross=263.00 discount=0.00 gift=0.00 shipping=0.00 final=263.00
- u_h: gross=615.00 discount=0.00 gift=0.00 shipping=0.00 final=615.00
- u_d: gross=609.00 discount=0.00 gift=0.00 shipping=0.00 final=609.00
- u_c: gross=240.00 discount=0.00 gift=0.00 shipping=0.00 final=240.00
- u_e: gross=600.00 discount=0.00 gift=0.00 shipping=0.00 final=600.00
- u_a: gross=135.00 discount=0.00 gift=0.00 shipping=0.00 final=135.00
- u_j: gross=270.00 discount=0.00 gift=0.00 shipping=0.00 final=270.00
- u_i: gross=439.00 discount=0.00 gift=0.00 shipping=0.00 final=439.00
