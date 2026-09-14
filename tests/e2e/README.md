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

- `gateway.bind = 127.0.0.1:<空闲端口>`（脚本先取一个空闲端口，再等待 `/api/gateway/status` 监听就绪）
- `gateway.reply_enabled = false`（安全红线：绝不发消息）
- `llm.enabled = false`（走确定性 `RuleParser`，不连网）
- `round.priority_window` 设为固定窗口（`1_600_000_000_000` ~ `1_600_003_600_000`）
- `round.priority_users` 设为 `["成员01"]`（与占位成员表对齐）
- `members.cache_path` 指向临时目录
- 成员来源通过 `PAIGU_MEMBERS_SEED_PATH`（指向临时不存在文件）与
  `PAIGU_MEMBERS_EXAMPLE_PATH`（指向入库 `data/members.example.json`）固定为 `example`

模拟页通过 `?ws=ws://127.0.0.1:<gateway 端口>` 连接**真实反向 WS**；发送的是 OneBot 11
群消息事件，走与真实链路相同的 Gateway→Pipeline→Engine→展示管道。

## 用例

每例前经页面「重置模拟会话」（同时清空服务端与页面状态）。

1. **WS 连接成功**：Gateway 监听且 `clients >= 1`；页面 `window.__simWsReady === true`。
2. **常规排谷（经真实 WS）**：`成员01`（预存）→ `排 通行证 结城理 1` → `Applied`，`pass_sp/v_jcl` 首格为 `成员01`（`/api/display` + 页面表格）。
3. **非排谷忽略**：`今天天气不错` → `Ignored`。
4. **时段拒绝**：非预存 `成员02` + 偏移落在优先窗口内 → `Rejected`，原因指向「优先时段/预存」。
5. **预存优先**：非预存 `成员02` 先排同一变体，再预存 `成员01` 排同一变体 → 该变体首格变为 `成员01`。
6. **`scripts/sim-run --speed 0` 回放**：JSONL 经 WS 发送 → 统计 `Applied=2`，排位首格正确。
7. **录制→重放一致**：`sim-record --record`（stdin 操作序列）写 `record.jsonl`，`--replay --speed 0` 重放，两次 `/api/display`（去掉 `claim_id` 等易变字段）结果一致。
8. **`/sim` 静态资源 200**：`/sim`、`/common.js`、`/sim.js`、`/display.css` 均返回 200。
9. **`PUT /api/config` 冲突 409**：携带陈旧 `revision` 提交 → `409` + `stale_revision`。
10. **`/api/members` 回退**：seed 缺失时返回 `source=example` 的 5 个占位成员。
11. **`remote` 数据源**：本地静态服务提供 `rounds/<id>/current`（无 `.json`），`/display?source=remote` 渲染远程快照。

断言以 `/api/display`（服务端日志/展示）为主，页面转录/表格文本为辅。
