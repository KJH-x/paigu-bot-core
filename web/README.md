# web · 排谷机器人前端（A4）

纯 vanilla JS、无构建、无框架。四页：展示 / 管理 / 模拟器 / 重放步进。只依赖本地 HTTP API（DESIGN §5）。

## 文件

| 文件 | 作用 |
|---|---|
| `index.html` | 入口导航 + `/api/health` 检查 |
| `display.html` / `display.js` | 展示页：排位表 + 消息流 + who-whats + 状态 |
| `admin.html` / `admin.js` | 管理面板：config 编辑（revision 乐观并发）、拉取群成员 |
| `sim.html` / `sim.js` | 模拟器：身份、时间偏移、发送、转录、实时排位 |
| `replay.html` / `replay.js` / `replay.css` | 重放步进查看器：步进/播放、累积排位与状态差异高亮、列视图、Mermaid、核对条、双向联动 |
| `display.css` | 各页共用样式（含排位渲染器、状态差异高亮） |
| `common.js` | 共用：API 封装、数据归一化、keyed diff 排位渲染器、smart-scroll、成员子集 |

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
- 数据源：`local`（同源 API）/ `remote`（静态 JSON）。remote 依次尝试 `<base>/rounds/<round_id>/current.json`、`<base>/current.json`（`<base>` 本身以 `.json` 结尾则直接用）。

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
