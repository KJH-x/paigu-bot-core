# real-xlsx —— 真实结果表（xlsx）解析 · 逐格验证 · 重放查看器

用户提供的两份真实排谷结果表（**唯一权威数据**）：

| 文件 | 样本 | Sheet1（分配结果） | Sheet2 | Sheet3 |
|---|---|---|---|---|
| `月行水上.xlsx` | 月行水上 | 特典（259元）/ 特典卡组-校园凭证 / 通行认证SP / 人事部简历SP / 风尚速递SP / 单领 | 报盒矩阵 | 空 |
| `覆雪于冬.xlsx` | 覆雪于冬 | 通行认证53.0 / 风尚速递 / 特典 / 单领 | 报盒矩阵 | 按人账单（who-whats 来源） |

> `覆雪于冬.xlsx` 的 `xl/styles.xml` 会让 openpyxl 报 `TypeError: Fill() takes no arguments`，故 `parse_xlsx.py` 对该文件直读 zip XML。

## 已确认的语义（用户口径）

- **Sheet1 是唯一权威**：行 = 商品 / 变体；**列向右 = 认购顺序（append）**；同一列多行 = **并发**（允许乱序）。
- 每个 section 由「小标题行」定义：`单价`/`调价`/`原价`/`折后价`/`数量`/`认购计数`/`cn` 决定字段列与认购起始列。
- **代理**：`Anyone_A（代Anyone_B）`（全/半角括号均可）→ 身份 = `Anyone_A`，**显示统一半角括号** `Anyone_A(代Anyone_B)`。
- 全角冒号 `：` 归一为 `:`（如 `code：015` → `code:015`）。
- **价格先不处理**（调价滞后）；只做「角色 + 商品（或其变体）」的排序。
- 一步 = 一条消息，可自主合并同一人并发的多个商品（含拼团 + 单领 + 代排，代排消息由 `Anyone_A` 发出）。
- Sheet3 who-whats：按 canonical display 分组，`A(代B)`、`A(代C)`、裸 `A` **互不合并**。

## 产物（每个样本目录）

```
real-xlsx/<样本>/
  sections.json            # 归一化：section / item / variant / 有序 claims
  messages.json            # 一步一条消息（按 pos+user 合并并发）
  who_whats.json           # Sheet3 的 who-whats（仅覆雪于冬非空）
  round_config.json        # 变体感知引擎输入
  queue.jsonl              # 变体感知引擎输入（重建的群消息）
  expected_allocation.json # item_id|variant_id -> 有序 user 列表
  tech_map.json            # 技术名 -> 展示名映射
  out/                     # simulate 产物：report.md / result.json / outcomes.jsonl
```

脚本：`parse_xlsx.py`（xlsx→JSON）、`build_fixtures.py`（JSON→夹具 + 跑引擎 + 逐格比对）、`build_replay_view.py`（→ `viewer/` 数据）。

## 引擎映射（变体感知）

| xlsx | 引擎 |
|---|---|
| `split_variants` section（行=变体） | 1 个 base `Item{kind:split}`，`variants[]`（`capacity`=该变体认购数） |
| `split_group` section（整组 N 份） | 1 个 `Item{kind:split, box_size:N}` |
| `single` section（单领） | 每行 1 个 `Item{kind:single, max_quantity:N}` |
| 列 = 认购顺序 | 消息按列位置递增；同列 = 同时间戳（并发） |
| `A(代B)` | `user_id=A`，`nickname=A(代B)` |

为规避规则解析器的子串匹配冲突，变体在 round_config 内使用技术名 `{变体名}（S{section}）`，查看器用 `tech_map.json` 还原真实名。

## 验证结果（逐格一致）

```
月行水上: 47 条消息全部生效；cells pass=32 fail=0
覆雪于冬: 20 条消息全部生效；cells pass=15 fail=0
ALL PASS
```

即每个商品/变体/单领的**最终占位序列与 xlsx 左→右顺序完全一致**。

复现：

```powershell
python simulation-corpus/real-xlsx/parse_xlsx.py
python simulation-corpus/real-xlsx/build_fixtures.py
python simulation-corpus/real-xlsx/build_replay_view.py
```

## 重放查看器

```powershell
cd viewer
python -m http.server 8095
# 浏览器打开 http://127.0.0.1:8095/
```

- 顶部「样本」下拉可切换 `月行水上 / 覆雪于冬`（数据在 `viewer/data/`）。
- 左：消息顺序；右：表格 / Mermaid；步进 + 双向高亮；底部 who-whats；核对条 PASS。

## 已知限制

1. **包尾**：xlsx 未记录包尾占用（如 月行水上 人事部简历SP / 风尚速递SP 的剩余变体为空），故按 xlsx 原样，未生成包尾消息。
2. **价格**：`调价/折后价/原价` 已解析入 JSON，但未参与分配/结算。
3. **跨 section 并发合并**：`(pos, user)` 合并为一条消息是启发式（xlsx 无全局时间轴），不影响各变体内部顺序。
4. **Sheet3 名称拆分**：对 `证件照1眼罩-银灰1…` 这类拼接串用正则 best-effort 拆分，`qty` 以尾部数字计；异常项保留原串。
5. **同变体跨 section 同名**：引擎侧用 `（S{n}）` 技术名消歧，展示名由 `tech_map.json` 还原。
