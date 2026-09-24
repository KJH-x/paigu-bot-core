# web · 排谷机器人前端（A4）

纯 vanilla JS、无构建、无框架。页面覆盖入口、开团、设置、排谷展示、结算/下单、模拟器与重放；只依赖本地 HTTP API（DESIGN §5）。

## 文件

| 文件 | 作用 |
|---|---|
| `index.html` | 入口导航 + `/api/health` 检查 |
| `display.html` / `display.js` | 展示页：排位表 + 消息流 + who-whats + 状态 |
| `round.html` / `round.js` | 开团页：轮次库、阶段窗口、商品目录与变体调价 |
| `settings.html` / `settings.js` | 设置页：gateway / LLM / display / 成员 CN 等非流程配置 |
| `admin.html` / `admin.js` | 兼容管理入口 |
| `settlement.html` / `settlement.js` / `settlement.css` | 结算试算、排包与下单方案 |
| `sim.html` / `sim.js` | 模拟器：身份、时间偏移、发送、转录、实时排位 |
| `replay.html` / `replay.js` / `replay.css` | 重放步进查看器：步进/播放、累积排位与状态差异高亮、列视图、Mermaid、核对条、双向联动 |
| `display.css` / `theme.css` / `app.css` | 共用基础样式、语义 token、工作流外壳与响应式布局 |
| `newlook.css` | 最后加载的方形视觉层：无圆角、分层边框、高对比焦点、明暗主题统一覆盖 |
| `common.js` / `shell.js` | API 与数据归一化、keyed diff、smart-scroll、工作流导航与主题切换 |

## 打开方式

推荐从 `127.0.0.1` 打开（API CORS 允许 `http://127.0.0.1:*`）：

```powershell
cd C:\_CustomPrograms\Pages\paigu-bot-core\web
python -m http.server 8098
# 浏览器打开 http://127.0.0.1:8098/
```

后端（A3）就绪时监听 `127.0.0.1:21081`：

```powershell
# 仓库根
cargo run
```

API 未就绪时页面不崩：展示页显示「API 未就绪…自动重试」横幅并每 5s 重试；管理页提示无法载入配置；模拟器提示 API 不可用且成员回退内置子集（DESIGN §8）。

## 配置覆盖（优先级从高到低）

1. URL 查询参数：`?api=<base>`、`?source=local|remote`、`?remote=<base>`、`?round=<round_id>`
2. `localStorage`（在页面上改过 API/数据源后记住）
3. 页面内联：`window.PAIGU_WEB = { apiBase, dataSource, remoteBaseUrl, refreshMs }`
4. 默认：`API_BASE = http://127.0.0.1:21081`，`refreshMs = 5000`
示例：`display.html?source=remote&remote=https://example.pages.dev/rounds/月行水上`

## 展示页行为

- 5s 增量轮询 `GET /api/display?since=<version>`；`changed=false` 且版本不变时跳过重渲染。
- keyed diff：单元格以 `item|variant#box:slot` 为 key，只更新变化格并高亮（3s 后淡出）；消息、who-whats、状态同样按 key 复用 DOM 节点。
- smart-scroll：消息容器贴近底部（阈值 60px）时自动跟随；否则不动滚动，只显示「有新内容 N 条」按钮。
- 不打断交互：不使用 `innerHTML` 重建容器，不移动无关节点；文字仅在变化时写入，尽量不破坏划词/点击。
- 数据源：`local`（同源 API）/ `remote`（静态 JSON）。remote 依次尝试 `<base>/rounds/<round_id>/current`、`<base>/current`（无扩展名；`<base>` 本身以 `.json` 结尾则直接用）。

## 管理面板

- `GET /api/config` 载入 → 编辑 → `PUT /api/config`（带 `revision`）；HTTP 409 显示冲突提示，需「重新载入」后再编辑。
- 「从磁盘重载」= `POST /api/config/reload`。
- 商品编辑器：items / variants 增删改（`item_id/name/kind/aliases`，variant `variant_id/name/capacity/aliases`）。
- 时段窗口：`datetime-local` ↔ `priority_window.start_ms/end_ms`（本地时区，end 独占）。
- 「拉取群成员」= `POST /api/members/refresh`，再 `GET /api/members` 渲染；昵称按 POLICY §2 去括号备注展示。

## 模拟器

- 身份来自 `GET /api/members`（真实群成员子集，失败回退 DESIGN §8 内置子集）；支持「＋ 新建/自定义身份」（`user_id/nickname/is_admin/是否预存`）。
- 时间偏移格式 `±DD HH MM SS`（可省略高位：`MM SS`、`SS` 亦可）；只作为该消息的 `offset_ms` 提交，服务器「现在」不变（POLICY §6）。
- 发送 `POST /api/sim/message`，转录记录 `outcome` 与 `version`，并即时刷新排位；`Ctrl/Cmd+Enter` 快捷发送。
- 「重置模拟会话」= `POST /api/sim/reset`。

## 重放步进（`replay.html`，路由 `/replay`）

- **数据源（二选一，本页选静态文件）**：默认 `fetch('replay_view.json')`（与页面同目录，可选），失败时回退到 `replay.js` 内嵌的占位示例；也可用「载入 JSON」选择任意 `replay_view.json`。**不从 `/api/display` 增量构造 steps**（`/api/display?since=` 只给最近一次 `changed` 且缓存有限，无法稳定还原逐步累积态）。schema 见旧 `viewer/README.md`（已随 `viewer/` 删除，字段：`items/messages/steps/who_whats/expected`）。
- **复用**：表格视图直接调用 `common.js` 的 `createBoardRenderer(host, { persistent: true })`（`persistent` 让高亮保持到下一步，而非 3s 淡出），并把 replay 的 `steps[].board`/`changed` 经 `normalizeBoard`/`normalizeChanged` 转成渲染器输入；差异高亮图标+颜色+边框沿用 `display.css`（`changed-new/release/forward/tail/admin`）。`persistent` 仅在重放页启用，展示页行为不变。
- **视图**：表格（累积排位 + 本步差异）、列视图（列=消息，行=商品/变体）、Mermaid（商品→变体→用户，当前步变更节点/边高亮）。
- **交互**：首/上/播放/下/末 + `←/→/空格/Home/End`；点单元格反查「最后一次修改该格」的步并聚焦；点消息/列头跳到该步；底部 who-whats 可折叠；存在 `expected` 时显示 PASS/FAIL 核对条。

## 降级约定

- 所有 `fetch` 带 8s 超时与错误分类（网络 / HTTP 状态）。
- 配置、成员、展示数据任一失败都不抛到控制台导致崩溃；页面显示中文横幅并自动重试。
- `web/**` 仅前端；不修改 `src/llm/**`、`src/gateway/**`、`src/settings.rs`、`simulation-corpus/**`；`/replay` 静态路由由 `src/api/mod.rs` 提供。

## 工作流外壳与双色主题（2026-09-24 重做）

新增三个共享文件（6 页统一）：

| 文件 | 说明 |
|---|---|
| `theme.css` | 设计 token：`:root`（浅色） + `[data-theme="dark"]`（深色），并覆盖 display/replay/settlement 中的硬编码浅色 |
| `app.css` | 外壳/Stepper/KPI/响应式与组件皮肤 |
| `shell.js` | 统一外壳：品牌 + **工作流 Stepper** + 状态徽标（API / NapCat `clients` / `version` / `locked` / 阶段）+ 主题切换（持久化 `paigu.theme`，首绘前生效防闪烁）+ `/api/workflow` 轮询（5s）+ 移动端底部阶段 Tab + 面板切换 |

- **流程来源**：只读 `GET /api/workflow`（阶段 `phase`/`phase_label`、`locked`、`version`、`claims`、`gateway.bound_addr`、`settlement_configured`、`priority_window`）。
- **Stepper 五步**：`开团→admin` · `排谷→/ 与 sim` · `结算→settlement` · `下单/锁定→settlement#order` · `复盘→replay`；`phase=null`（未配置阶段）时显示「阶段未配置」并高亮「排谷」。
- **响应式**：`≥1200` 桌面多列；`768–1199` 平板（Stepper 可横滚）；`<768` 移动（底部阶段 Tab + 单列 + 吸附操作条）。
- **图标**：全部内联 SVG；**零新增依赖**（replay 的 Mermaid 视图由 `shell.js` 内置的 Mermaid-lite 渲染，不再依赖 CDN）。
- 页面脚本（`display.js`/`admin.js`/`sim.js`/`replay.js`/`settlement.js`）**未改数据契约**，所有既有 `id`/`class` 保留。

## Newlook 方形视觉层（2026-09-25）

`newlook.css` 在各页最后加载，不改功能脚本与 DOM 契约。设计取向是高信息密度的操作台，而不是卡片化消费产品：

- 所有组件零圆角；层级主要靠 1px 实线、灰阶表面和少量 3–4px 强调边，不用悬浮阴影制造深度；
- 颜色是语义 token：蓝色只用于主要操作与当前模块，成功/警告/错误同时使用文字和左侧色条，不只依赖色相；
- 键盘焦点使用高对比双层轮廓，并保留 `forced-colors`；动效服从 `prefers-reduced-motion`；
- 表格、编号、状态与价格使用等宽/等宽数字，移动端保持单列和底部流程导航。

取舍参考：[Carbon 的颜色分层与 token](https://carbondesignsystem.com/elements/color/overview/)、[Carbon spacing](https://carbondesignsystem.com/elements/spacing/overview/)、[W3C Focus Appearance](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance)、[W3C Non-text Contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast)、[GOV.UK focus states](https://design-system.service.gov.uk/get-started/focus-states/)。
