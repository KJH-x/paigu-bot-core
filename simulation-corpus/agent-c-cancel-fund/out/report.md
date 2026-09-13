# 模拟验证报告: es_2026_05

- group_id: 10003
- 消息总数: 57
- 生效(Applied): 45
- 拒绝(Rejected): 0
- 需确认(NeedConfirm): 0
- 忽略(Ignored): 2
- 未支持(Unsupported): 10
- 重复(Duplicate): 0

## 购物金优先权 (Eligibility)

- user=u_c level=10 note=购物金(自述) valid_from=2026-05-08T12:00:05+00:00
- user=u_e level=10 note=购物金(自述) valid_from=2026-05-08T12:00:05+00:00
- user=u_g level=10 note=购物金(自述) valid_from=2026-05-08T12:00:05.700+00:00
- user=u_c level=10 note=优先权 valid_from=2026-05-08T12:00:06.200+00:00
- user=u_b level=10 note=优先权 valid_from=2026-05-08T12:00:28.200+00:00

## 逐条消息结果

| seq | msg | user | status | text | detail |
|---|---|---|---|---|---|
| 1 | c1 | u_admin | Applied | /开团 es_2026_05 | 开团(模拟中默认已开) |
| 2 | c2 | u_a | Applied | 排燐音吧唧2 | claim: badge_rinnex2[normal] |
| 3 | c3 | u_b | Applied | 燐音吧唧1 | claim: badge_rinnex1[normal] |
| 4 | c4 | u_d | Applied | 蓝良吧唧2 | claim: badge_ranx2[normal] |
| 5 | c5 | u_f | Applied | 排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 6 | c6 | u_h | Applied | 特典色纸1 | claim: postcardx1[normal] |
| 7 | c7 | u_a | Applied | 再排燐音立牌1 | claim: stand_rinnex1[normal] |
| 8 | c8 | u_c | Applied | 燐音吧唧1 我有购物金 | claim: badge_rinnex1[normal] |
| 9 | c9 | u_e | Applied | 用购物金排立牌2 | claim: stand_rinnex2[normal] |
| 10 | c10 | u_g | Applied | 蓝良吧唧1 购物金 | claim: badge_ranx1[normal] |
| 11 | c11 | u_admin | Applied | /加优先 丙 燐音吧唧 | 授权优先权 user=u_c level=10 |
| 12 | c12 | u_b | Ignored | 管理员能给我也加个优先权吗 | 无法识别为排谷/撤销意图 |
| 13 | c13 | u_admin | Unsupported | /设置优惠 燐音吧唧 满3减100 | 优惠规则解析暂未进入模拟重放 |
| 14 | c14 | u_a | Applied | 撤燐音1 | cancel: item=Some("badge_rinne") qty=Some(1) |
| 15 | c15 | u_b | Unsupported | 改燐音吧唧为2 | 改单意图未在校验层实现 |
| 16 | c16 | u_c | Applied | 撤燐音1 | cancel: item=Some("badge_rinne") qty=Some(1) |
| 17 | c17 | u_d | Applied | 蓝良不要了 | cancel: item=Some("badge_ran") qty=Some(1) |
| 18 | c18 | u_e | Applied | 撤立牌1 | cancel: item=Some("stand_rinne") qty=Some(1) |
| 19 | c19 | u_h | Applied | 色纸不要了 | cancel: item=Some("postcard") qty=Some(1) |
| 20 | c20 | u_f | Applied | 取消 | cancel: item=None qty=None |
| 21 | c21 | u_a | Applied | 包尾燐音吧唧 | claim: badge_rinnex1[tail_locked] |
| 22 | c22 | u_b | Applied | 包盒燐音吧唧 | claim: badge_rinnex10[full_box] |
| 23 | c23 | u_d | Applied | 重新排蓝良吧唧1 | claim: badge_ranx1[normal] |
| 24 | c24 | u_g | Applied | 撤蓝良1 | cancel: item=Some("badge_ran") qty=Some(1) |
| 25 | c25 | u_e | Applied | 蓝良吧唧2 | claim: badge_ranx2[normal] |
| 26 | c26 | u_admin | Unsupported | /锁位 燐音吧唧 | 管理员锁位/修正暂未进入模拟重放 |
| 27 | c27 | u_f | Applied | 再排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 28 | c28 | u_a | Applied | 撤燐音两个 | cancel: item=Some("badge_rinne") qty=Some(2) |
| 29 | c29 | u_c | Unsupported | 改燐音吧唧为1 | 改单意图未在校验层实现 |
| 30 | c30 | u_b | Unsupported | 立牌改成单领 | 改单意图未在校验层实现 |
| 31 | c31 | u_h | Applied | 再要色纸2 | claim: postcardx2[normal] |
| 32 | c32 | u_e | Applied | 撤立牌1 | cancel: item=Some("stand_rinne") qty=Some(1) |
| 33 | c33 | u_admin | Unsupported | /修正 甲 燐音吧唧 2 | 管理员锁位/修正暂未进入模拟重放 |
| 34 | c34 | u_g | Applied | 退了 | cancel: item=None qty=None |
| 35 | c35 | u_d | Unsupported | 蓝良吧唧改成2 | 改单意图未在校验层实现 |
| 36 | c36 | u_a | Applied | 燐音吧唧3 | claim: badge_rinnex3[normal] |
| 37 | c37 | u_f | Applied | 撤燐音1 | cancel: item=Some("badge_rinne") qty=Some(1) |
| 38 | c38 | u_e | Applied | 立牌1 购物金优先 | claim: stand_rinnex1[normal] |
| 39 | c39 | u_h | Applied | 特典色纸1 | claim: postcardx1[normal] |
| 40 | c40 | u_admin | Unsupported | /设置优惠 立牌 满2减200 | 优惠规则解析暂未进入模拟重放 |
| 41 | c41 | u_b | Ignored | 免减吗 | 无法识别为排谷/撤销意图 |
| 42 | c42 | u_c | Applied | 我有购物金 排蓝良1 | claim: badge_ranx1[normal] |
| 43 | c43 | u_a | Applied | 撤 | cancel: item=None qty=None |
| 44 | c44 | u_d | Applied | 撤 | cancel: item=None qty=None |
| 45 | c45 | u_e | Applied | 撤 | cancel: item=None qty=None |
| 46 | c46 | u_f | Applied | 撤 | cancel: item=None qty=None |
| 47 | c47 | u_h | Applied | 撤 | cancel: item=None qty=None |
| 48 | c48 | u_g | Applied | 再排燐音吧唧1 购物金 | claim: badge_rinnex1[normal] |
| 49 | c49 | u_b | Unsupported | 燐音吧唧改成2 | 改单意图未在校验层实现 |
| 50 | c50 | u_admin | Applied | /加优先 乙 燐音吧唧 | 授权优先权 user=u_b level=10 |
| 51 | c51 | u_a | Applied | 蓝良吧唧1 | claim: badge_ranx1[normal] |
| 52 | c52 | u_c | Applied | 撤蓝良1 | cancel: item=Some("badge_ran") qty=Some(1) |
| 53 | c53 | u_d | Applied | 包尾蓝良吧唧 | claim: badge_ranx1[tail_locked] |
| 54 | c54 | u_e | Applied | 立牌2 | claim: stand_rinnex2[normal] |
| 55 | c55 | u_f | Applied | 色纸1 | claim: postcardx1[normal] |
| 56 | c56 | u_admin | Applied | /结团 es_2026_05 | 结团 |
| 57 | c57 | u_admin | Unsupported | /导出 es_2026_05 | 导出不在模拟范围内 |

## 最终排结果

### 特典色纸 (postcard, gift)

- box1: u_h
- box2: u_h
- box3: u_f

### 燐音立牌 (stand_rinne, single)

- 单领: u_ex2, u_ax1

### 燐音吧唧 (badge_rinne, split)

- box1: u_g | u_b | · | · | · | · | · | · | · | ·
- box2: u_b | u_b | u_b | u_b | u_b | u_b | u_b | u_b | u_b | u_b

### 蓝良吧唧 (badge_ran, split)

- box1: u_e | u_e | u_d | u_a | · | · | · | · | · | ·
- box2: u_d | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED

## 结算

- u_f: gross=0.00 discount=0.00 gift=0.00 shipping=0.00 final=0.00
- u_g: gross=45.00 discount=0.00 gift=0.00 shipping=0.00 final=45.00
- u_b: gross=495.00 discount=0.00 gift=0.00 shipping=0.00 final=495.00
- u_d: gross=90.00 discount=0.00 gift=0.00 shipping=0.00 final=90.00
- u_a: gross=105.00 discount=0.00 gift=0.00 shipping=0.00 final=105.00
- u_e: gross=210.00 discount=0.00 gift=0.00 shipping=0.00 final=210.00
- u_h: gross=0.00 discount=0.00 gift=0.00 shipping=0.00 final=0.00
