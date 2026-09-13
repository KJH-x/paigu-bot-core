# 排谷分配重放步进查看器

纯前端、无构建步骤的 replay 步进查看器。用于回看「排谷」重放结果：逐条消息步进，
在表格 / Mermaid 两种视图里高亮每一步的分配变化，并支持单元格 ↔ 消息双向联动。

## 文件

| 文件 | 说明 |
|---|---|
| `index.html` | 页面入口，可直接双击用 `file://` 打开 |
| `viewer.js` | 全部逻辑（vanilla JS，无框架），内含示例数据回退 |
| `style.css` | 样式 |
| `sample_replay.json` | 示例数据（3 个商品、13 步、含 who_whats 与 expected） |
| `replay_view.json` | 默认数据（= `data/月行水上.json`） |
| `data/月行水上.json` | 真实样本：月行水上（47 步，32 核对分组） |
| `data/覆雪于冬.json` | 真实样本：覆雪于冬（20 步，15 核对分组，含 who-whats 19 人） |
| `data/真实聊天-月行水上.json` | 真实群聊复现：月行水上（14 步，两张 QQ 截图语料） |
| `README.md` | 本文件 |

顶部「样本」下拉可切换 `月行水上 / 覆雪于冬 / 真实聊天·月行水上`（读取 `data/<样本>.json`）。

## 如何打开

1. **最简单**：双击 `index.html`。
   - `file://` 下浏览器通常会拦截 `fetch('replay_view.json')`，此时会自动回退到
     `viewer.js` 内嵌的示例数据（与 `sample_replay.json` 等价），页面照常渲染。
2. **带真实数据（推荐）**：用自带脚本起本地服务（会读取 `replay_view.json` 与 `data/*.json`）：
   ```powershell
   pwsh -File viewer\serve.ps1          # 默认 8095
   # 或指定端口： pwsh -File viewer\serve.ps1 9000
   # 浏览器访问 http://127.0.0.1:8095/
   ```
   等价的原始命令：
   ```powershell
   cd viewer ; python -m http.server 8095 --bind 127.0.0.1
   ```
   也可以直接点右上角「载入 JSON」，手动选择任意 `replay_view.json`（无需服务）。
3. Mermaid 视图使用 CDN：`https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js`。
   若 CDN 不可用，Mermaid 区域会显示提示，**表格视图仍完全可用**。

## 界面与操作

- **顶部**：标题、总步数、当前步号；首步 / 上一步 / 播放 / 下一步 / 末步、播放速度（0.5x–4x）、载入 JSON。
- **左侧**：一步一条消息，显示 `#seq 显示名：text [status]`；当前步高亮，点击任意行跳转。
- **右侧**：
  - **表格视图（默认）**：按 `section → 商品 → 变体` 分组，每个变体一行，按 slot 顺序渲染单元格（显示 user，空位为虚线框）。单领商品按 `item_id|` 渲染。
  - **列视图**：矩阵，**列 = 认购顺序/消息**（表头 `#seq 用户`），**行 = 商品/变体**，格子 = 该消息认领的变体与用户。当前步所在列高亮；点击列头或任意格子跳到该步。适合与 xlsx 的「列=认购顺序」逐列对照。
  - **Mermaid 视图**：`商品 → 变体 → 用户`（无变体的单领为 `商品 → 用户`），当前步 `changed` 涉及的节点与边高亮。
- **高亮图例**（图标 + 颜色 + 边框，不只靠颜色）：

  | reason | 图标 | 含义 |
  |---|---|---|
  | `NewClaimFilled` | ▲ | 新增填入（绿） |
  | `CancelReleased` | ▼ | 撤销释放（红） |
  | `AutoMovedForward` | ↷ | 自动前移（黄） |
  | `TailSegmentCreated` / `TailSegmentUpdated` | ⛓ | 包尾段（紫） |
  | `AdminFixed` / `AdminUnlocked` | ✚ | 管理员固定/解锁（蓝） |

- **双向联动**：
  - 点击右侧某个单元格 → 反查「最后一次改变该格」的 step，跳过去并高亮该格与左侧消息。
  - 点击左侧消息 → 跳到该步，右侧高亮该步所有变更格。
- **键盘**：`←` / `→` 上一步 / 下一步，`空格` 播放 / 暂停，`Home` / `End` 首步 / 末步。
- **底部 Who-Whats**：`display | 身份 | 明细(name ×qty)`，可折叠；`who_whats` 为空时不显示。
- **核对条**：存在 `expected` 时，按 key 比对「期望 user 序列 vs 当前最终态」，一致显示 `PASS`，否则列出差异。

## 输入 schema：`replay_view.json`

```json
{
  "sample_id": "月行水上",
  "title": "月行水上",
  "items": [
    { "item_id": "月行水上/特典卡组-校园凭证", "name": "特典卡组-校园凭证", "kind": "split",
      "section": "特典卡组-校园凭证",
      "variants": [ { "variant_id": "整套", "name": "整套", "capacity": 5 } ] },
    { "item_id": "月行水上/通行证SP-月行水上", "name": "通行证SP-月行水上", "kind": "single",
      "section": "单领", "variants": [] }
  ],
  "messages": [
    { "seq": 1, "pos": 1, "user": "SIM", "display": "SIM", "text": "排整套",
      "status": "Applied", "detail": "claim: ...",
      "claims": [ { "item_id": "月行水上/特典卡组-校园凭证", "item_name": "特典卡组-校园凭证",
                    "variant_id": "整套", "variant_name": "整套", "qty": 1 } ] }
  ],
  "steps": [
    { "index": 0, "message_seq": 1,
      "board": {
        "月行水上/特典卡组-校园凭证|整套": [ { "slot": 1, "user": "SIM", "status": "filled" } ]
      },
      "changed": [
        { "item_id": "月行水上/特典卡组-校园凭证", "variant_id": "整套", "slot": 1,
          "before": null, "after": "SIM", "reason": "NewClaimFilled" }
      ]
    }
  ],
  "who_whats": [ { "display": "Dele", "identity": "Dele", "items": [ { "name": "凛冬立牌", "qty": 1 } ] } ],
  "expected": { "月行水上/特典卡组-校园凭证|整套": ["SIM", "嘟嘟", "DKLA", "cz", "静边城"] }
}
```

### 字段说明

- `items[]`
  - `kind`：`split`（拆分，有 `variants`）/ `single`（单领，`variants` 为空）/ 其它（`gift` / `shipping` / `adjustment`，按普通商品渲染）。
  - `section`：表格分组的一级标题；缺省为「未分组」。
  - `variants[]`：`variant_id`、`name`、`capacity`（该变体的槽位容量，用于渲染空位数量）。
- `messages[]`
  - `seq`：消息序号，用于与 `steps[].message_seq` 关联。
  - `display`：展示名（缺省回退 `user`）；`text` 原文；`status` 状态（`Applied` / `Rejected` / `NeedConfirm` / `Ignored` / `Unsupported` / `Duplicate` 等）。
  - `claims[]`：该消息解析出的认领行（供扩展展示，核心渲染不依赖）。
- `steps[]`：一步一条消息。
  - `message_seq`：对应 `messages[].seq`。
  - `board`：**该步之后**的板面，key 为 `item_id|variant_id`（无变体用空串，如 `"<item_id>|"`），value 为 `{slot, user, status}` 数组。
    - 允许只给「本步涉及」的 key；查看器会按步累积，未出现的 key 沿用上一步，key 内的 slot 列表按「整体替换」处理（因此删掉某 slot 即表示该格变空）。
    - 单领 item 的 board 用 `"<item_id>|"`，slot 从 1 开始，`status` 为 `filled`。
  - `changed[]`：本步变化明细。`reason` ∈ `NewClaimFilled` / `CancelReleased` / `AutoMovedForward` / `TailSegmentCreated` / `AdminFixed`（另兼容 `TailSegmentUpdated` / `AdminUnlocked` / `RecomputedByRuleChange`）。
    - `before` / `after` 为用户名或 `null`（空）。
- `who_whats[]`：`display`、`identity`、`items[{name,qty}]`。
- `expected`（可选）：key 为 `item_id|variant_id`，value 为按 slot 顺序的 user 序列，用于核对最终态。

## 自测记录

- `sample_replay.json` 含 3 个 item（`split` 带变体 + `single`）、13 步、`who_whats`、`expected`。
- 直接双击 `index.html`（`file://`）时 fetch 本地 json 失败 → 回退内嵌示例，页面渲染正常。
- 已验证交互：步进（首/上/播放/下/末 + 键盘）、视图切换、changed 高亮（新增/释放/前移/包尾）、
  单元格点击反查最近修改步、左侧消息点击联动、who-whats 折叠、expected 核对条。
