# web · 排谷机器人前端（A4）

纯 vanilla JS、无构建、无框架。七个工作区统一导航：总览、轮次与商品、排谷、结算与下单、复盘、成员、系统设置；模拟器页已移除。依赖本地 HTTP API 或只读远程快照。

## 文件

| 文件 | 作用 |
|---|---|
| `index.html` | 入口导航 + `/api/health` 检查 |
| `display.html` / `display.js` | 展示页：排位表 + 消息流 + who-whats + 状态 |
| `round.html` / `round.js` | 开团页：轮次库、阶段窗口、商品目录与子项调价 |
| `settings.html` / `settings.js` | 系统设置四个 hash 分区：接入 / 消息理解 / 展示默认各自保存，数据维护承载磁盘重载与私有备份 |
| `members.html` / `members.js` | 成员称呼、别名及缓存来源；分别保存；LLM 建议先预览再选入草稿 |
| `admin.html` | 兼容管理入口，保留查询与锚点转总览，不加载旧 admin.js |
| `msglog.js` / `message-editor.js` | 排谷原文只读日志与统一动作编辑器；远程只读不请求本机私有日志 |
| `settlement.html` / `settlement.js` / `settlement.css` | 结算试算、排包与下单方案 |
| `replay.html` / `replay.js` / `replay.css` | 重放步进查看器：步进/播放、累积排位与状态差异高亮、列视图、Mermaid、核对条、双向联动 |
| `display.css` / `theme.css` / `app.css` | 共用基础样式、语义 token、工作流外壳与响应式布局 |
| `newlook.css` | 方形视觉层：无圆角、分层边框、高对比焦点、明暗主题统一覆盖 |
| `workbench.css` | 最后加载的桌面工作台布局：统一顶栏、宽内容区、对齐分栏与紧凑表单 |
| `board-layout.js` / `board.css` | 排表姓名宽度模式、实际字体测量、共享盒列轨道和上下状态标记（展示/复盘最后加载） |
| `common.js` / `shell.js` | API 与数据归一化、keyed diff、smart-scroll、七区导航与主题；共用草稿注册/三选离开保护、未知锚点恢复 |

## 打开方式

单领和等待名单的姓名标签同样遵循宽度模式，数量单独显示在姓名下方，完整姓名/数量保留在提示与无障碍说明。

桌面验收画布为 **1920×1080、100%**。各页保持信息量，统一工具栏、标题层级、边框和内区滚动；开团页左侧轮次/阶段、右侧目录，结算页左侧规则、右侧下单表。金额展示/输入使用元，保存仍转换为整数分。子项名称列按约 10 个汉字宽度分配，其余空间留给别名。

排表默认「三字宽」，姓名区按实际字体三个中文字测量，空位同宽，长姓名裁切并保留完整悬停提示。切换「全长」后，同一商品每盒按该盒各行最长姓名统一格宽，各盒列起点对齐，不同盒不互相撑宽；格间 2px、盒间 4px，超宽内容在商品内横滚。选择保存在 `localStorage["paigu.board-width"]`，跨页面/标签页共享。上沿 2px 盒色条，下沿 4px 锁定/预留/包尾/变化标记，不占姓名宽度；标记有形状区别和无障碍说明。复盘表格按全部载入步骤定宽，切步不跳列；列视图按各消息列最长姓名定宽。模拟器已被功能提交移除，不恢复。

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
2. 页面显式内联 `apiBase`；HTTP(S) 部署默认当前同源
3. 仅 file 模式回退本机保存的 API 偏好，最后默认 `http://127.0.0.1:21081`
4. 展示来源/远程地址/刷新间隔独立沿用其展示偏好；`P.pageHref` 为跨工作区链接保留 API，不将远程静态快照当作可写 API
示例：`display.html?source=remote&remote=https://example.pages.dev/rounds/月行水上`

## 展示页行为

消息范围可选全部/待确认/拒绝/失败/人工修订；正在定位的旧条修订后即使不符筛选也暂留原位，明确清除定位后移出。编辑器去优先或阶段设置时，草稿仅存本标签 sessionStorage（两小时），URL 不带成员名/消息或动作对象。返回和浏览器后退按实例/轮次/记录版本复核并恢复视图；冲突只读保留草稿。保存/放弃后清理，关闭/重载仍提醒；不自动激活轮次。只读复盘须主动“去排谷处理”进入同一记录，导入文件与远程来源不提供可写入口。

- 5s 增量轮询 `GET /api/display?since=<version>`；`changed=false` 且版本不变时跳过重渲染。
- keyed diff：单元格以 `item|subitem#box:slot` 为 key，只更新变化格并高亮（3s 后淡出）；消息、who-whats、状态同样按 key 复用 DOM 节点。
- smart-scroll：消息容器贴近底部（阈值 60px）时自动跟随；否则不动滚动，只显示「有新内容 N 条」按钮。
- 不打断交互：不使用 `innerHTML` 重建容器，不移动无关节点；文字仅在变化时写入，尽量不破坏划词/点击。
- 数据源：`local`（同源 API）/ `remote`（静态 JSON）。remote 依次尝试 `<base>/rounds/<round_id>/current`、`<base>/current`（无扩展名；`<base>` 本身以 `.json` 结尾则直接用）。

## 管理面板

- `GET /api/config` 载入 → 编辑 → `PUT /api/config`（带 `revision`）；HTTP 409 显示冲突提示，需「重新载入」后再编辑。
- 「从磁盘重载」= `POST /api/config/reload`。
- 商品编辑器：items / subitems 增删改（`item_id/name/kind/aliases`，subitem `subitem_id/name/capacity/aliases`）。
- 时段窗口：`datetime-local` ↔ `priority_window.start_ms/end_ms`（本地时区，end 独占）。
- 「拉取群成员」携带读取时的轮次、配置版本和名单指纹，等待后仍须同源同版本才原子写缓存；断开/过期保留旧名单。称呼建议也校验三项，先预览选择后填入本地草稿，不自动保存；停用模型时不会外发。来源设置变更后明确显示旧列表并停用拉取/建议，重新读取后再操作。

## 测试注入（HTTP，仅测试轮）

- 模拟器页（`sim.html`/`sim.js`）**已移除**；测试消息改由 HTTP 端点注入，且**仅测试轮**（`round.category == "test"`）可用。
- 单条 `POST /api/sim/message`、批量 `POST /api/sim/messages`（裸数组或 `{messages:[…]}`）。每条为完整消息 `{user_id|id, text, timestamp, nickname?, is_admin?, message_id?, group_id?}`，`timestamp` **逐条出现**。
- 缺字段/非测试群的消息**逐条 `Dropped`**（不整批拒绝）；非测试轮调用返回 `400 {"error":"not_a_test_round"}`。
- 「重置」= `POST /api/sim/reset`（手动）。

## 重放步进（`replay.html`，路由 `/replay`）

- **数据源**：默认 `GET /api/replay/view` 读取当前激活轮次的实际管线消息和事件步骤，默认停在最新一步；成员明细随当前步变化。轮次切换和新消息触发刷新，空数据/错误不回退到示例。手动「载入 JSON」仅用于查看外部复盘文件；点击“当前轮次 · 刷新”回到实际数据。旧服务未提供接口时仅显示当前快照，明确提示逐步历史需要更新服务。
- **复用**：表格视图直接调用 `common.js` 的 `createBoardRenderer(host, { persistent: true })`（`persistent` 让高亮保持到下一步，而非 3s 淡出），并把 replay 的 `steps[].board`/`changed` 经 `normalizeBoard`/`normalizeChanged` 转成渲染器输入；差异高亮图标+颜色+边框沿用 `display.css`（`changed-new/release/fwd/tail/admin`）。`persistent` 仅在重放页启用，展示页行为不变。
- **视图**：表格（累积排位 + 本步差异）、列视图（列=消息，行=商品/子项）、Mermaid（商品→子项→用户，当前步变更节点/边高亮）。
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
- **现行导航七区**：每页一次点击可到其余六区，当前页由实际工作区决定，与业务阶段解耦；未配置/断线/锁定不影响导航。`/` 保持排谷默认入口；七页短路由与 `/web/*.html` 可用。旧 `settings#sec-members`、`settings#sec-messages`、`index#sec-msglog` 到唯一新入口；API、监听、接收器分别显示，零接收器不是绿色。
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
