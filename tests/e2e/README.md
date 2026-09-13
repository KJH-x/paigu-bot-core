# 排谷机器人 · 端到端测试（A5）

Node `.mjs` + Playwright 的端到端测试。脚本**自带服务启动**：先 `cargo build`，用临时配置拉起
`target/debug/paigu-bot-core.exe run`，再用 Chromium 打开模拟器页面并驱动「选身份 → 设偏移 → 发消息」，
断言排位与状态。全程只打本地 API/页面，**不连接真实群、不真连 LLM**。

## 依赖安装（在本仓库内，独立于 browser-agent）

```powershell
npm install
npx playwright install chromium
```

`node_modules/` 已被 `.gitignore` 忽略；`package.json` / `package-lock.json` 入库。

## 运行

```powershell
node tests/e2e/sim.mjs
# 或
npm run test:e2e
# 或（一键脚本）
pwsh tests/e2e/run.ps1
```

退出码：全部通过 `0`，否则 `1`。逐例输出 `PASS <case>` / `FAIL <case> <原因>`。

## 环境变量

| 变量 | 作用 |
|---|---|
| `PAIGU_HTTP_PORT` | 不读取；脚本自动选择一个空闲高位端口并注入子进程 |
| `RUST_LOG` | 透传给服务进程，默认 `warn` |
| `PAIGU_CONFIG_PATH` / `PAIGU_WEB_DIR` | 由脚本设置为临时配置与仓库 `web/`，无需手动指定 |

## 测试配置（临时，不落库）

复制 `config.example.json` 到系统临时目录并改写：

- `gateway.bind = 127.0.0.1:0`（不占用真实端口）
- `gateway.reply_enabled = false`（安全红线：绝不发消息）
- `llm.enabled = false`（走确定性 `RuleParser`，不连网）
- `round.priority_window` 设为固定窗口（`1_600_000_000_000` ~ `1_600_003_600_000`）
- `members.cache_path` 指向临时目录

## 用例

每例前 `POST /api/sim/reset`。

1. **常规排谷**：身份 `SIM`（预存）→ `排 通行证 结城理 1` → 断言 `Applied`，且 `pass_sp/v_jcl` 首格为 `SIM`（API + 页面表格）。
2. **非排谷忽略**：`今天天气不错` → 断言 `Ignored`（API + 转录）。
3. **时段拒绝**：非预存 `齐布/阿布` + 偏移落在优先窗口内 → 断言 `Rejected`，原因指向「优先时段/预存」。
4. **预存优先**：非预存 `齐布/阿布` 先排同一变体，再预存 `SIM` 排同一变体 → 断言该变体首格变为 `SIM`。

断言以 HTTP API（`/api/sim/message` 响应）为主，页面元素/表格文本为辅。

## 已知问题（遗留）

- `/sim` 路由返回 HTML，但其相对静态资源（`common.js` / `sim.js` / `display.css`）返回 404：
  当前 `src/api` 只把 `web/` 挂在 `/web` 前缀下。测试会先尝试 `/sim?api=...`，检测到页面脚本
  未加载时自动回退到 `/web/sim.html?api=...` 并打印 `NOTE`。修复路由（在根路径提供静态资源）后
  测试将自动回到 `/sim`，无需改动本脚本。
