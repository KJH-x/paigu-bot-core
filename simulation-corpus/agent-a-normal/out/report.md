# 模拟验证报告: es_2026_05

- group_id: 10001
- 消息总数: 52
- 生效(Applied): 52
- 拒绝(Rejected): 0
- 需确认(NeedConfirm): 0
- 忽略(Ignored): 0
- 未支持(Unsupported): 0
- 重复(Duplicate): 0

## 购物金优先权 (Eligibility)

- user=u_kanata level=10 note=购物金 valid_from=2026-05-08T12:00:01+00:00
- user=u_mayoi level=10 note=购物金 valid_from=2026-05-08T12:00:01.001+00:00

## 逐条消息结果

| seq | msg | user | status | text | detail |
|---|---|---|---|---|---|
| 1 | a1 | u_tsukasa | Applied | /加优先 用户=@奏汰 等级=10 范围=全部 备注=购物金 | 授权优先权 user=u_kanata level=10 |
| 2 | a2 | u_tsukasa | Applied | /加优先 用户=@真宵 等级=10 范围=全部 备注=购物金 | 授权优先权 user=u_mayoi level=10 |
| 3 | a3 | u_rinne | Applied | 排燐音吧唧2 谢谢团长~ | claim: badge_rinnex2[normal] |
| 4 | a4 | u_ran | Applied | 蓝良吧唧1 | claim: badge_ranx1[normal] |
| 5 | a5 | u_tsukasa | Applied | 燐音亚克力包盒 | claim: acrylic_rinnex8[full_box] |
| 6 | a6 | u_ran | Applied | 蓝良立牌单领1 | claim: stand_ranx1[normal] |
| 7 | a7 | u_tsukasa | Applied | 帮排燐音1个谢谢 | claim: badge_rinnex1[normal] |
| 8 | a8 | u_aoi | Applied | 排蓝良吧唧2 | claim: badge_ranx2[normal] |
| 9 | a9 | u_hiyori | Applied | 燐音吧唧3 手速！ | claim: badge_rinnex3[normal] |
| 10 | a10 | u_jun | Applied | 亚克力1 | claim: acrylic_rinnex1[normal] |
| 11 | a11 | u_jun | Applied | 排rinne吧唧2 | claim: badge_rinnex2[normal] |
| 12 | a12 | u_niki | Applied | 蓝良+1 | claim: badge_ranx1[normal] |
| 13 | a13 | u_hiyori | Applied | 立牌单领2 | claim: stand_ranx2[normal] |
| 14 | a14 | u_sora | Applied | 燐音+1 蹲 | claim: badge_rinnex1[normal] |
| 15 | a15 | u_sora | Applied | 燐音亚克力2 | claim: acrylic_rinnex2[normal] |
| 16 | a16 | u_aoi | Applied | 吧唧燐音1 手慢无 | claim: badge_rinnex1[normal] |
| 17 | a17 | u_sora | Applied | 蓝良吧唧两个 | claim: badge_ranx2[normal] |
| 18 | a18 | u_sora | Applied | @凛音 我亚克力1 帮看下 | claim: acrylic_rinnex1[normal] |
| 19 | a19 | u_niki | Applied | 燐音吧唧来两个捏 | claim: badge_rinnex2[normal] |
| 20 | a20 | u_jun | Applied | 要蓝良吧唧1 | claim: badge_ranx1[normal] |
| 21 | a21 | u_sora | Applied | 蓝良立牌1 | claim: stand_ranx1[normal] |
| 22 | a22 | u_kanata | Applied | 我有购物金 排燐音吧唧2 | claim: badge_rinnex2[normal] |
| 23 | a23 | u_rinne | Applied | 蓝良吧唧1 谢谢 | claim: badge_ranx1[normal] |
| 24 | a24 | u_rinne | Applied | 亚克力+1 | claim: acrylic_rinnex1[normal] |
| 25 | a25 | u_mayoi | Applied | 购物金用户 燐音吧唧1 | claim: badge_rinnex1[normal] |
| 26 | a26 | u_aoi | Applied | 立牌+1 | claim: stand_ranx1[normal] |
| 27 | a27 | u_hiyori | Applied | 帮排蓝良吧唧2 | claim: badge_ranx2[normal] |
| 28 | a28 | u_jun | Applied | 单领蓝良立牌3 | claim: stand_ranx3[normal] |
| 29 | a29 | u_rinne | Applied | 再排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 30 | a30 | u_jun | Applied | 燐音吧唧代牌1 | claim: badge_rinnex1[normal] |
| 31 | a31 | u_mayoi | Applied | 购物金 蓝良吧唧2 | claim: badge_ranx2[normal] |
| 32 | a32 | u_niki | Applied | 蓝良立牌2 谢谢 | claim: stand_ranx2[normal] |
| 33 | a33 | u_aoi | Applied | 亚克力1 | claim: acrylic_rinnex1[normal] |
| 34 | a34 | u_mayoi | Applied | 购物金 立牌4 | claim: stand_ranx4[normal] |
| 35 | a35 | u_tsukasa | Applied | 蓝良吧唧+1 | claim: badge_ranx1[normal] |
| 36 | a36 | u_rinne | Applied | 蓝良立牌2 | claim: stand_ranx2[normal] |
| 37 | a37 | u_hiyori | Applied | 帮排亚克力2 | claim: acrylic_rinnex2[normal] |
| 38 | a38 | u_tsukasa | Applied | 立牌3 | claim: stand_ranx3[normal] |
| 39 | a39 | u_kanata | Applied | 立牌2 | claim: stand_ranx2[normal] |
| 40 | a40 | u_tsukasa | Applied | 蓝良吧唧包尾 | claim: badge_ranx1[tail_locked] |
| 41 | a41 | u_rinne | Applied | 排燐音吧唧1 蓝良吧唧1 | claim: badge_rinnex1[normal], badge_ranx1[normal] |
| 42 | a42 | u_niki | Applied | 立牌单领1 谢谢 | claim: stand_ranx1[normal] |
| 43 | a43 | u_aoi | Applied | 亚克力1 | claim: acrylic_rinnex1[normal] |
| 44 | a44 | u_ran | Applied | 蓝良吧唧1 | claim: badge_ranx1[normal] |
| 45 | a45 | u_sora | Applied | 蓝良吧唧1 亚克力1 | claim: badge_ranx1[normal], acrylic_rinnex1[normal] |
| 46 | a46 | u_mayoi | Applied | 再排燐音吧唧1 | claim: badge_rinnex1[normal] |
| 47 | a47 | u_hiyori | Applied | 燐音吧唧1 蓝良吧唧1 | claim: badge_rinnex1[normal], badge_ranx1[normal] |
| 48 | a48 | u_jun | Applied | 亚克力1 | claim: acrylic_rinnex1[normal] |
| 49 | a49 | u_niki | Applied | 燐音吧唧1 | claim: badge_rinnex1[normal] |
| 50 | a50 | u_kanata | Applied | 购物金 燐音吧唧1 | claim: badge_rinnex1[normal] |
| 51 | a51 | u_ran | Applied | 立牌单领1 | claim: stand_ranx1[normal] |
| 52 | a52 | u_aoi | Applied | 燐音吧唧1 | claim: badge_rinnex1[normal] |

## 最终排结果

### 燐音吧唧 (badge_rinne, split)

- box1: u_kanata | u_kanata | u_mayoi | u_mayoi | u_kanata | u_rinne | u_rinne | u_tsukasa | u_hiyori | u_hiyori
- box2: u_hiyori | u_jun | u_jun | u_sora | u_aoi | u_niki | u_niki | u_rinne | u_jun | u_rinne
- box3: u_hiyori | u_niki | u_aoi | · | · | · | · | · | · | ·

### 燐音亚克力 (acrylic_rinne, split)

- box1: u_tsukasa | u_tsukasa | u_tsukasa | u_tsukasa | u_tsukasa | u_tsukasa | u_tsukasa | u_tsukasa
- box2: u_jun | u_sora | u_sora | u_sora | u_rinne | u_aoi | u_hiyori | u_hiyori
- box3: u_aoi | u_sora | u_jun | · | · | · | · | ·

### 蓝良吧唧 (badge_ran, split)

- box1: u_mayoi | u_mayoi | u_ran | u_aoi | u_aoi | u_niki | u_sora | u_sora | u_jun | u_rinne
- box2: u_hiyori | u_hiyori | u_tsukasa | u_rinne | u_ran | u_sora | u_hiyori | · | · | ·
- box3: u_tsukasa | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED | LOCKED

### 蓝良立牌 (stand_ran, single)

- 单领: u_mayoix4, u_kanatax2, u_ranx1, u_hiyorix2, u_sorax1, u_aoix1, u_junx3, u_nikix2, u_rinnex2, u_tsukasax2
- 等待(waiting): u_tsukasax1, u_nikix1, u_ranx1

## 结算

- u_aoi: gross=316.00 discount=0.00 gift=0.00 shipping=0.00 final=316.00
- u_mayoi: gross=420.00 discount=0.00 gift=0.00 shipping=0.00 final=420.00
- u_kanata: gross=255.00 discount=0.00 gift=0.00 shipping=0.00 final=255.00
- u_jun: gross=436.00 discount=0.00 gift=0.00 shipping=0.00 final=436.00
- u_sora: gross=392.00 discount=0.00 gift=0.00 shipping=0.00 final=392.00
- u_hiyori: gross=511.00 discount=0.00 gift=0.00 shipping=0.00 final=511.00
- u_niki: gross=300.00 discount=0.00 gift=0.00 shipping=0.00 final=300.00
- u_ran: gross=150.00 discount=0.00 gift=0.00 shipping=0.00 final=150.00
- u_rinne: gross=428.00 discount=0.00 gift=0.00 shipping=0.00 final=428.00
- u_tsukasa: gross=559.00 discount=0.00 gift=0.00 shipping=0.00 final=559.00
