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
| `replay.html` / `replay.js` / `replay.css` | 重放步进查看器：步进/播放、累积排位与状态差异高亮、列视图、Mermaid、核对条、双向联动 |
| `display.css` / `theme.css` / `app.css` | 共用基础样式、语义 token、工作流外壳与响应式布局 |
| `newlook.css` | 方形视觉层：无圆角、分层边框、高对比焦点、明暗主题统一覆盖 |
| `workbench.css` | 最后加载的桌面工作台布局：统一顶栏、宽内容区、对齐分栏与紧凑表单 |
| `common.js` / `shell.js` | API 与数据归一化、keyed diff、smart-scroll、工作流导航与主题切换 |

## 打开方式

桌面验收画布为 **1920×1080、100%**。各页保持信息量，统一工具栏、标题层级、边框和内区滚动；开团页左侧轮次/阶段、右侧目录，结算页左侧规则、右侧下单表。金额展示/输入使用元，保存仍转换为整数分。变体名称列按约 10 个汉字宽度分配，其余空间留给别名。

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

## 测试注入（HTTP，仅测试轮）

- 模拟器页（`sim.html`/`sim.js`）**已移除**；测试消息改由 HTTP 端点注入，且**仅测试轮**（`round.category == "test"`）可用。
- 单条 `POST /api/sim/message`、批量 `POST /api/sim/messages`（裸数组或 `{messages:[…]}`）。每条为完整消息 `{user_id|id, text, timestamp, nickname?, is_admin?, message_id?, group_id?}`，`timestamp` **逐条出现**。
- 缺字段/非测试群的消息**逐条 `Dropped`**（不整批拒绝）；非测试轮调用返回 `400 {"error":"not_a_test_round"}`。
- 「重置」= `POST /api/sim/reset`（手动）。

## 重放步进（`replay.html`，路由 `/replay`）

- **数据源**：默认 `GET /api/replay/view` 读取当前激活轮次的实际管线消息和事件步骤，默认停在最新一步；成员明细随当前步变化。轮次切换和新消息触发刷新，空数据/错误不回退到示例。手动「载入 JSON」仅用于查看外部复盘文件；点击“当前轮次 · 刷新”回到实际数据。旧服务未提供接口时仅显示当前快照，明确提示逐步历史需要更新服务。
- **复用**：表格视图直接调用 `common.js` 的 `createBoardRenderer(host, { persistent: true })`（`persistent` 让高亮保持到下一步，而非 3s 淡出），并把 replay 的 `steps[].board`/`changed` 经 `normalizeBoard`/`normalizeChanged` 转成渲染器输入；差异高亮图标+颜色+边框沿用 `display.css`（`changed-new/release/fwd/tail/admin`）。`persistent` 仅在重放页启用，展示页行为不变。
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
- **Stepper 五步**：`开团→round` · `排谷→/` · `结算→settlement` · `下单/锁定→settlement#order` · `复盘→replay`；`phase=null`（未配置阶段）时显示「阶段未配置」并高亮「排谷」。
- **响应式**：`≥1200` 桌面多列；`768–1199` 平板（Stepper 可横滚）；`<768` 移动（底部阶段 Tab + 单列 + 吸附操作条）。
- **图标**：全部内联 SVG；**零新增依赖**（replay 的 Mermaid 视图由 `shell.js` 内置的 Mermaid-lite 渲染，不再依赖 CDN）。
- 页面脚本（`display.js`/`admin.js`/`replay.js`/`settlement.js`/`round.js`/`settings.js`）**未改数据契约**，所有既有 `id`/`class` 保留。

## Newlook 方形视觉层（2026-09-25）

`newlook.css` 在各页最后加载，不改功能脚本与 DOM 契约。设计取向是连续、可扫描的任务工作台，而不是一组彼此漂浮的表单卡片：

- 所有组件零圆角；内容画布限制为 1180px，顶栏、局部导航和同级面板无缝衔接，外层网格只用于标示应用边界；
- 用「应用顶栏 → 局部导航 → 编号功能区 → 内嵌卡片/表格 → 编辑控件」五级样式建立层级；仅主要操作和独立商品卡使用无模糊硬阴影；
- 橙、蓝、紫、青、红分别标记接入/轮次、展示、LLM/重放、成员/结算、日志/风险等功能类别；成功、警告、错误仍同时使用文字与边框，不只依赖色相；
- 表单采用统一 12 栏轨道，短数值、ID、标题、URL 按预期内容长度分配宽度；可编辑控件用实底和粗下边线，只读控件用斜纹与虚线，开关开启后显示整块色洗；
- 键盘焦点使用高对比双层轮廓，并保留 `forced-colors`；动效服从 `prefers-reduced-motion`；移动端回到单列、保留底部流程导航。

取舍参考：[Are.na Sander 的高对比、内联编辑与快速导航](https://www.are.na/editorial/introducing-sander-our-new-web-client)、[Bloomberg 的客户导向产品设计](https://www.bloomberg.com/company/stories/bloombergs-customer-centric-design-ethos/)、[GOV.UK 固定宽度输入](https://design-system.service.gov.uk/components/text-input/)、[GOV.UK 响应式布局](https://design-system.service.gov.uk/styles/layout/)、[Carbon 的颜色分层与 token](https://carbondesignsystem.com/elements/color/overview/)、[W3C Focus Appearance](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance) 与 [W3C Non-text Contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast)。
