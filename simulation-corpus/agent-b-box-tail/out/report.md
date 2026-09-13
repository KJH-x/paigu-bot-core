# 模拟验证报告: es_2026_05

- group_id: 10002
- 消息总数: 49
- 生效(Applied): 49
- 拒绝(Rejected): 0
- 需确认(NeedConfirm): 0
- 忽略(Ignored): 0
- 未支持(Unsupported): 0
- 重复(Duplicate): 0

## 购物金优先权 (Eligibility)

- user=u_e level=10 note=购物金(自述) valid_from=2026-05-08T12:00:03+00:00

## 逐条消息结果

| seq | msg | user | status | text | detail |
|---|---|---|---|---|---|
| 1 | b1 | u_b | Applied | 燐音吧唧排1个 | claim: badge_rinnex1[normal] |
| 2 | b2 | u_c | Applied | 排rinne吧唧2 | claim: badge_rinnex2[normal] |
| 3 | b3 | u_f | Applied | 罐徽排1 | claim: can_badgex1[normal] |
| 4 | b4 | u_d | Applied | 蓝良吧唧排3 | claim: badge_ranx3[normal] |
| 5 | b5 | u_a | Applied | 阿甲 燐音吧唧要3个 | claim: badge_rinnex3[normal] |
| 6 | b6 | u_b | Applied | 蓝良立牌单领1 | claim: stand_ranx1[normal] |
| 7 | b7 | u_c | Applied | 罐徽包尾2 | claim: can_badgex2[tail_locked] |
| 8 | b8 | u_e | Applied | 吧唧燐音排2 购物金 | claim: badge_rinnex2[normal] |
| 9 | b9 | u_d | Applied | 立牌排2个 | claim: stand_ranx2[normal] |
| 10 | b10 | u_g | Applied | 罐徽排2 | claim: can_badgex2[normal] |
| 11 | b11 | u_d | Applied | 燐音吧唧整盒 | claim: badge_rinnex10[full_box] |
| 12 | b12 | u_h | Applied | 罐徽再来1 | claim: can_badgex1[normal] |
| 13 | b13 | u_f | Applied | 吧唧排1 | claim: badge_rinnex1[normal] |
| 14 | b14 | u_a | Applied | 蓝良立牌要3 | claim: stand_ranx3[normal] |
| 15 | b15 | u_b | Applied | 罐徽包尾巴1 | claim: can_badgex1[tail_locked] |
| 16 | b16 | u_g | Applied | 再排个燐音吧唧 | claim: badge_rinnex1[normal] |
| 17 | b17 | u_d | Applied | 罐徽留尾1 | claim: can_badgex1[tail_locked] |
| 18 | b18 | u_h | Applied | rinne +1 | claim: badge_rinnex1[normal] |
| 19 | b19 | u_f | Applied | 立牌1 | claim: stand_ranx1[normal] |
| 20 | b20 | u_e | Applied | 蓝良 ran 2个 购物金 | claim: badge_ranx2[normal] |
| 21 | b21 | u_h | Applied | 罐徽端了3 | claim: can_badgex3[tail_locked] |
| 22 | b22 | u_g | Applied | 蓝良吧唧端盒2 | claim: badge_ranx2[tail_locked] |
| 23 | b23 | u_c | Applied | 罐徽包尾8 | claim: can_badgex8[tail_locked] |
| 24 | b24 | u_h | Applied | 蓝良吧唧包个尾1 | claim: badge_ranx1[tail_locked] |
| 25 | b25 | u_b | Applied | 撤销罐徽包尾巴1 | cancel: item=Some("can_badge") qty=Some(1) |
| 26 | b26 | u_b | Applied | 蓝良吧唧要尾2 | claim: badge_ranx2[tail_locked] |
| 27 | b27 | u_f | Applied | 蓝良吧唧锁列1 | claim: badge_ranx1[column_locked] |
| 28 | b28 | u_g | Applied | 蓝良吧唧锁一整列1 | claim: badge_ranx1[column_locked] |
| 29 | b29 | u_f | Applied | 蓝良吧唧一盒全包 | claim: badge_ranx10[full_box] |
| 30 | b30 | u_a | Applied | 蓝良吧唧包一盒 | claim: badge_ranx10[full_box] |
| 31 | b31 | u_e | Applied | 燐音吧唧包一盒 | claim: badge_rinnex10[full_box] |
| 32 | b32 | u_g | Applied | 罐徽包盒 | claim: can_badgex6[full_box] |
| 33 | b33 | u_b | Applied | 蓝良吧唧排1 | claim: badge_ranx1[normal] |
| 34 | b34 | u_d | Applied | 蓝良吧唧再来2 谢谢 | claim: badge_ranx2[normal] |
| 35 | b35 | u_f | Applied | 蓝良立牌再来1 | claim: stand_ranx1[normal] |
| 36 | b36 | u_h | Applied | 蓝良吧唧排1 | claim: badge_ranx1[normal] |
| 37 | b37 | u_c | Applied | 燐音吧唧排2 | claim: badge_rinnex2[normal] |
| 38 | b38 | u_e | Applied | 燐音吧唧再来1 购物金 | claim: badge_rinnex1[normal] |
| 39 | b39 | u_g | Applied | 罐徽排1 | claim: can_badgex1[normal] |
| 40 | b40 | u_b | Applied | 燐音吧唧排1 求求了 | claim: badge_rinnex1[normal] |
| 41 | b41 | u_a | Applied | 蓝良吧唧排1 | claim: badge_ranx1[normal] |
| 42 | b42 | u_d | Applied | 罐徽再来1 | claim: can_badgex1[normal] |
| 43 | b43 | u_f | Applied | 燐音吧唧锁列1 | claim: badge_rinnex1[column_locked] |
| 44 | b44 | u_h | Applied | 蓝良立牌单领2 | claim: stand_ranx2[normal] |
| 45 | b45 | u_c | Applied | 罐徽排1 别漏我 | claim: can_badgex1[normal] |
| 46 | b46 | u_g | Applied | 蓝良吧唧排2 | claim: badge_ranx2[normal] |
| 47 | b47 | u_e | Applied | 燐音罐徽排2 | claim: can_badgex2[normal] |
| 48 | b48 | u_a | Applied | 立牌排1 | claim: stand_ranx1[normal] |
| 49 | b49 | u_h | Applied | 罐徽包尾1 试试 | claim: can_badgex1[tail_locked] |

## 最终排结果

### 燐音吧唧 (badge_rinne, split)

- box1: u_e | u_e | u_e | u_b | u_c | u_c | u_a | u_a | u_a | u_f
- box2: u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e | u_e
- box3: u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d | u_d
- box4: u_g | u_h | u_c | u_c | u_b | u_f | · | · | · | ·

### 蓝良吧唧 (badge_ran, split)

- box1: u_e | u_e | u_d | u_d | u_d | u_f | u_g | u_b | u_d | u_d
- box2: u_g | u_g | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED
- box3: u_h | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED
- box4: u_b | u_b | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED
- box5: u_f | u_f | u_f | u_f | u_f | u_f | u_f | u_f | u_f | u_f
- box6: u_a | u_a | u_a | u_a | u_a | u_a | u_a | u_a | u_a | u_a
- box7: u_h | u_a | u_g | u_g | · | · | · | · | · | ·

### 燐音罐徽 (can_badge, split)

- box1: u_e | u_e | u_f | u_g | u_g | u_h
- box2: u_c | u_c | LOCKED | LOCKED | LOCKED | LOCKED
- box3: u_d | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED
- box4: u_h | u_h | u_h | LOCKED | LOCKED | LOCKED
- box5: u_c | u_c | u_c | u_c | u_c | u_c
- box6: u_g | u_g | u_g | u_g | u_g | u_g
- box7: u_g | u_d | u_c | · | · | ·
- box8: u_h | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED

### 蓝良立牌 (stand_ran, single)

- 单领: u_bx1, u_dx2, u_ax3, u_fx1, u_fx1, u_hx2, u_ax1

## 结算

- u_g: gross=540.00 discount=0.00 gift=0.00 shipping=0.00 final=540.00
- u_b: gross=285.00 discount=0.00 gift=0.00 shipping=0.00 final=285.00
- u_h: gross=405.00 discount=0.00 gift=0.00 shipping=0.00 final=405.00
- u_d: gross=855.00 discount=0.00 gift=0.00 shipping=0.00 final=855.00
- u_e: gross=735.00 discount=0.00 gift=0.00 shipping=0.00 final=735.00
- u_f: gross=735.00 discount=0.00 gift=0.00 shipping=0.00 final=735.00
- u_c: gross=450.00 discount=0.00 gift=0.00 shipping=0.00 final=450.00
- u_a: gross=870.00 discount=0.00 gift=0.00 shipping=0.00 final=870.00
