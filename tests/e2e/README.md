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

公开开发的统一入口见 [DEVELOPMENT](../../docs/DEVELOPMENT.md)：`npm run check:full` 包含完整桌面、隔离核心及脚本/文档/隐私。私有参考/语料需显式选择，不把缺失输入算通过。

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
| `PAIGU_E2E_FILTER` | 可选：按用例名称子串只运行专项；零匹配报错。专项通过不等于全套通过，交付前需不设此变量运行全部 |
| `PAIGU_CONFIG_PATH` / `PAIGU_WEB_DIR` | 由脚本设置为临时配置与仓库 `web/`，无需手动指定 |

## 测试配置（临时，不落库）

“当前轮次复盘只读与真实步骤”还在首步/列视图中实际调用旧diff诊断及显式空目录预览，验证比较上下文、GET不继承预览、实时排位和当前步均不变；诊断比较当前已保存动作与实时板，不能因它可读就把磁盘历史暗中恢复到fresh投影。

`private-backup.mjs`使用真实隔离API/浏览器下载事件及原生文件选择，验证超JS安全整数原字节往返、草稿保护/明确预检应用/新消息冲突、文件与路径互斥/坏格式/64MiB以上不上传。取消、撤回隐私确认、API/配置变化和超时测试延迟真实端点响应，不能保存迟到文件；仅该用例在自有事件夹具中插入占位大整数。明暗截图等待主题120ms过渡完成，并检查输入文字实际对比度；1920×1080/100%，无横向溢出。

`draft-concurrency.mjs` 在真实隔离服务、关闭模型/发送的条件下，仅浏览器拦截代填响应，验证取消/连续请求乱序、切记录/轮次、并发修订、坏JSON/504/受控超时、真实订单锁定和锁后只读。`message-transactions.mjs` 连接本地假接收器捕获全部出站应用帧，保存/撤回/代填期间为0，不接入真实 NapCat。

`catalog-reachability.mjs` 验证四类商品、明暗、筛选及长目录首中末实际点击/焦点/吸顶遮挡；`workspace-navigation.mjs` 补四分区直开/刷新/后退前进和未知hash恢复；`correctness-gates.mjs` 补 Chromium IME 组合输入与受控粘贴（不访问系统剪贴板）。`message-identity.mjs` 用真实管理插入＋Dropped 制造序号分歧，再验证断线期划词/旧节点保留、重连原位修订。

只读表格回归：先构建，再运行 `python tests/read-only-corpus.py`（本机需有两份源工作簿和 openpyxl）。读取既有解析/夹具生成器，所有产物只写系统临时目录；仅将临时旧 variants 键转换为 subitems，不改预期排位/成员/数量，逐格比较并校验整个语料与源表哈希未变。私有输出不打印、不入库。

`settlement-review.mjs` 实际点击缺订单号/重复/价格/数量问题到输入，验证规则独立保存、上游变更标记旧试算、完整保留订单、显式复核、迟到自动保存来源 409 仍保留输入、复核不绕过数量锁定、明暗截图。正式页不写演示数据；布局测试直接向隔离测试轮注入占位订单。

`replay-graph.mjs` 用占位导入数据检查商品聚焦/分组、单领/等待、长混排姓名/特殊字符完整提示、键盘定位、字号下界、视区和步骤保留、旧绘图迟到守卫与零管理写入；步骤列表/列按钮可用 Space。`message-policy-return.mjs` 额外验证真实人工只改解释且事件数不变，当前轮复盘仍自动拉取新投影并保留步骤/定位。

`message-actions.mjs` 追加失效精确撤销链：原申购被前序人工忽略后，另一申购仍在；界面主动展开“撤销未命中”、保留原失效引用，不悄悄改撤别单。

`scoped-settings.mjs` 新增分区实际保存、其他区草稿保留、外部同区冲突、保存后导航、切实例与磁盘命令取消、浏览器重载提醒、模态 Tab/Escape、1920×1080 面板居中和字段轨道断言。消息与 G0 用例显式处理离开草稿，不绕过保护；测试须等实际 UI 锁态，而非只等 HTTP 响应。

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

排表专项见 `board-layout.mjs`：前端请求夹具只包含占位数据，验证三字实际测量、空位同宽、多盒/不同容量对齐、全长不裁切、商品内部横滚、上下状态标记、格子身份与高亮时效、跨页/跨标签页偏好共享、复盘历史定宽与点击追溯。全套当前注册33项，包括成员健康和`proxy-attribution.mjs`具名/群外归属、实际发言溯源、资格说明、展开/关闭、明暗主题及不改数据；新增`target-modify.mjs`覆盖管理员目标改单、原命令来源、键盘进出/滚动与执行开关历史边界。测试不恢复已移除的模拟器页。新增消息修订/补充/撤回、私有备份、G0 门禁及 `workspace-navigation.mjs` 七区42对真实点击、旧入口和上下文/明暗对比度检查。完整专项验收仍须逐项对照 docs/VERIFICATION-msg-ia-2026-10-18.md，不以本总数代替。下列17项为早期基础用例，后续专项见上文模块说明及主脚本实际注册，不代表完整清单。

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
