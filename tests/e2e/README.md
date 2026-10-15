# 排谷机器人 · 端到端测试（A5）

Node `.mjs` + Playwright 的端到端测试。脚本**自带服务启动**：先 `cargo build`，用临时配置拉起
`target/debug/paigu-bot-core.exe run`，再用 Chromium（**1920×1080、100%**）打开各页；测试消息经
`POST /api/sim/messages`（**仅测试轮**）注入，断言排位与状态。全程只打本地 API/页面，**不连接真实群、不真连 LLM**。

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
| `PAIGU_UI_CAPTURE_DIR` | 可选：保存 1920×1080、100% 页面与排表两种宽度/明暗主题截图 |
| `PAIGU_CONFIG_PATH` / `PAIGU_WEB_DIR` | 由脚本设置为临时配置与仓库 `web/`，无需手动指定 |

## 测试配置（临时，不落库）

复制 `config.example.json` 到系统临时目录并改写：

- `gateway.bind = 127.0.0.1:<空闲端口>`（脚本先取一个空闲端口，再等待 `/api/gateway/status` 监听就绪）
- `gateway.reply_enabled = false`（安全红线：绝不发消息）
- `gateway.test_group_id = 123456789`、`round.category = "test"`（**测试轮隔离**：测试注入仅测试轮可用）
- `llm.enabled = false`（走确定性 `RuleParser`，不连网）
- `round.priority_window` 设为固定窗口（`1_600_000_000_000` ~ `1_600_003_600_000`）
- `round.priority_users` 设为 `["成员01"]`（与占位成员表对齐）
- `members.cache_path` 指向临时目录
- 成员来源通过 `PAIGU_MEMBERS_SEED_PATH`（指向临时不存在文件）与
  `PAIGU_MEMBERS_EXAMPLE_PATH`（指向入库 `data/members.example.json`）固定为 `example`

测试消息经 `POST /api/sim/message(s)`（**仅测试轮**）注入，走与真实链路相同的 `Pipeline::process`；
脚本 `sim-run`/`sim-record` 经 `ws://127.0.0.1:<gateway 端口>`（**真实反向 WS**）发送 OneBot 11 群消息事件，走 Gateway→Pipeline→Engine→展示管道。

## 用例

排表专项见 `board-layout.mjs`：前端请求夹具只包含占位数据，验证三字实际测量、空位同宽、多盒/不同容量对齐、全长不裁切、商品内部横滚、上下状态标记、格子身份与高亮时效、跨页/跨标签页偏好共享、复盘历史定宽与点击追溯。全套当前为 17 项；测试不恢复已移除的模拟器页。

每例前经 `POST /api/sim/reset` 重置服务端状态。

1. **Gateway 监听**：`/api/gateway/status` `listening === true`。
2. **常规排谷（HTTP 注入）**：`成员01`（预存）→ `排 通行证 结城理 1` → `Applied`，`pass_sp/v_jcl` 首格为 `成员01`。
3. **非排谷忽略**：`今天天气不错` → `Ignored`。
4. **时段拒绝**：非预存 `成员02` + 时间落在优先窗口内 → `Rejected`，原因指向「优先时段/预存」。
5. **预存优先**：非预存 `成员02` 先排同一子项，再预存 `成员01` 排同一子项 → 该子项首格变为 `成员01`。
6. **批量注入 `/api/sim/messages`**：裸数组（1 合法 + 1 缺 `timestamp` + 1 非测试群）与 `{messages:[…]}` 包装；校验 `summary` 计数与逐条 `Dropped` 原因（`missing_timestamp` / `not_test_group`）。
7. **`scripts/sim-run --speed 0` 回放**：JSONL 经 WS 发送 → 统计 `Applied=2`，排位首格正确。
8. **录制→重放一致**：`sim-record --record`（stdin 操作序列）写 `record.jsonl`，`--replay --speed 0` 重放，两次 `/api/display`（去掉 `claim_id` 等易变字段）结果一致。
9. **静态资源 200 与 `/sim` 已移除**：`/`、`/common.js`、`/display.css`、`/round.html`、`/settings.html` 均 200；`/sim`、`/sim.js` 返回 404。
10. **`PUT /api/config` 冲突 409**：携带陈旧 `revision` 提交 → `409` + `stale_revision`。
11. **`/api/workflow` 工作流快照**：`round_id/phase/locked/version/gateway` 字段。
12. **当前轮次复盘只读与真实步骤**：注入 2 条 → `/api/replay/view` 不改实时排位，末步与实时一致；replay 页交互与旧服务降级。
13. **`POST /api/rounds/:id/setup` 开团前填写**：`dry_run` 不落盘、落盘回显 `category`、`round_id` 不一致 400。
14. **`/api/members` 回退**：seed 缺失时返回 `source=example` 的 5 个占位成员。
15. **桌面布局（1920×1080 / 100%）与元金额联动保存**：逐页断言 `devicePixelRatio===1`、画布 1920×1080、无横向溢出；元金额联动、别名切分、阶段归属。
16. **固定宽度、盒列对齐与复盘切步**：排表两模式、上下标记、多盒对齐和复盘稳定性（见专项说明）。
17. **`remote` 数据源**：本地静态服务提供 `rounds/<id>/current`（无 `.json`），`/display?source=remote` 渲染远程快照。

断言以 `/api/display`（服务端日志/展示）为主，页面转录/表格文本为辅。
