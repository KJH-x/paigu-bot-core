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
- `round.priority_users` 设为 `["成员01"]`（与占位成员表对齐）
- `members.cache_path` 指向临时目录
- 成员来源通过 `PAIGU_MEMBERS_SEED_PATH`（指向临时不存在文件）与
  `PAIGU_MEMBERS_EXAMPLE_PATH`（指向入库 `data/members.example.json`）固定为 `example`

## 用例

每例前 `POST /api/sim/reset`。

1. **常规排谷**：身份 `成员01`（预存）→ `排 通行证 结城理 1` → 断言 `Applied`，且 `pass_sp/v_jcl` 首格为 `成员01`（API + 页面表格）。
2. **非排谷忽略**：`今天天气不错` → 断言 `Ignored`（API + 转录）。
3. **时段拒绝**：非预存 `成员02` + 偏移落在优先窗口内 → 断言 `Rejected`，原因指向「优先时段/预存」。
4. **预存优先**：非预存 `成员02` 先排同一变体，再预存 `成员01` 排同一变体 → 断言该变体首格变为 `成员01`。
5. **`/sim` 静态资源 200**：`/sim`、`/common.js`、`/sim.js`、`/display.css` 均返回 200。
6. **`PUT /api/config` 冲突 409**：携带陈旧 `revision` 提交 → `409` + `stale_revision`。
7. **`/api/members` 回退**：seed 缺失时返回 `source=example` 的 5 个占位成员。
8. **`remote` 数据源**：本地静态服务提供 `rounds/<id>/current`（无 `.json`），`/display?source=remote` 渲染远程快照。

断言以 HTTP API（`/api/sim/message` 响应）为主，页面元素/表格文本为辅。

## 已知问题（遗留）

- 无（`web/` 已通过根路径静态回退提供相对资源，`/sim` 可直接使用）。
