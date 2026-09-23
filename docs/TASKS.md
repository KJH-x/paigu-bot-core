# 排谷机器人 · 任务拆分（TASKS）

> 每个子 agent 必须先读 [POLICY.md](./POLICY.md)、[DESIGN.md](./DESIGN.md)、[AGENT-RULES.md](./AGENT-RULES.md)，再动工。
> 文件所有权见 DESIGN §2；**不得越界修改**。

## A1 · Gateway（`src/gateway/**`）

**目标**：反向 WS 服务器，接 NapCat 事件，白名单/drop，只读动作回包。

交付：
- `src/gateway/mod.rs`
- `src/gateway/onebot.rs`：`RouteMessageEvent`（post_type/message_type/self_id/user_id/group_id/message_id/raw_message/message/sender），`RoutePolicy{whitelist_groups}`，`RouteKind{Drop,Message}`，`decide_route`（非白名单/非群消息/空 → Drop），`normalize_message`（CQ 码/段 → 纯文本），`sanitize`（脱敏日志）。
- `src/gateway/ws_server.rs`：`tokio_tungstenite` accept loop，绑定 `AppConfig.gateway.bind`；每连接 task；15s ping；解析文本帧 → `decide_route` → Drop 记日志 / Message 推入 `mpsc<IncomingEvent>`；处理 `echo` 动作响应（oneshot 表）；`send_action(action, params) -> Result<Value>`（校验 `allowed_actions`，**禁止 send_***）。
- `src/gateway/action.rs`：`get_group_member_list(group_id)` 等只读封装。

验收：`cargo test` 通过；单测覆盖 `decide_route`（白名单/非白名单/非 message/空）；`echo` 往返可用 mock 验证。

## A2 · LLM + 配置（`src/llm/**`、`src/config.rs`）

**目标**：DeepSeek 客户端 + 排谷流水线 + 热载配置。

交付：
- `src/config.rs`：`AppConfig`（按 DESIGN §3）、`ConfigStore`（RwLock + revision）、`load/save/reload`、`notify` 监听热载、env 覆盖。
- `src/llm/mod.rs`、`src/llm/client.rs`：OpenAI 兼容 `chat/completions`（reqwest），`base_url/model/api_key_env/timeout/max_tokens/temperature`，JSON 模式，超时/重试。
- `src/llm/prompt.rs`：按 DESIGN §6 组装 system prompt（含商品目录/预存用户/时段/输出 schema）。
- `src/llm/pipeline.rs`：`process(event, cfg, engine) -> PipelineOutcome`：
  幂等 → 规则快速路径(`RuleParser`) → LLM 清理/抽取 → `EventValidator` → 权限(时段/预存) → 分配 → 回复文本（`reply_enabled` 决定是否发送）。
  LLM 失败 → `fallback_to_rules` 回退。

验收：单测用 mock LLM（不真连网）覆盖：排谷成功、非排谷 Ignore、歧义 NeedConfirm、时段拒绝、预存优先；配置 revision 冲突 409 逻辑。

## A3 · HTTP API（`src/api/**`）

**目标**：按 DESIGN §5 提供路由（端口 21081）。

交付：
- `src/api/config_routes.rs`、`board_routes.rs`、`display_routes.rs`、`sim_routes.rs`、`member_routes.rs`；更新 `src/api/routes.rs` 挂载 + 静态页服务（`web/`）+ CORS。
- `sim_routes`：复用 A2 流水线；`offset_ms` 仅改该消息 `timestamp_ms`。
- `display_routes`：`since` 增量（board + messages + who_whats + status + changed）。

验收：curl/单测覆盖各路由；`/api/display?since=` 增量正确。

## A4 · 前端（`web/**`）

**目标**：display / admin / sim 三页，vanilla，可部署。

交付：
- `web/display.html|js|css`：排位表 + 消息流 + who-whats + 状态；**5s 增量轮询**；keyed diff 只改变化格；**不打断划词/点击/滚动**（smart-scroll）；数据源 local/remote 适配。
- `web/admin.html|js`：config 编辑（prompt/商品/预存/时段/白名单/LLM/展示），`revision` 乐观并发 + 冲突提示；“拉取群成员”按钮 + 成员表。
- `web/sim.html|js`：身份切换/新建（**真实群成员**）、时间偏移输入（`±DD HH MM SS`）、消息发送、转录、实时排位。
- `web/index.html`：入口导航。

验收：`python -m http.server` 本地可打开；与 API 联调。

## A5 · 测试（`tests/**`）

**目标**：Node `.mjs` Playwright e2e + Rust 集成测试。

交付：
- 本仓库内 `npm i -D playwright`（独立安装，避免借用 browser-agent 环境）；`tests/e2e/sim.mjs`：启动本地服务 → 打开 `/sim` → 选身份/设偏移/发消息 → 断言排位与状态（含时段拒绝、预存优先、非排谷忽略）。
- `tests/e2e/README.md`：运行方式。
- Rust 集成测试补充（gateway 路由 / config 热载）。

验收：`node tests/e2e/sim.mjs` 退出码 0；输出 PASS 摘要。

## A0 · 主（装配与验收）

- `src/main.rs`：新增 `run`（默认，跑 Gateway+API+Pipeline）子命令；保留 `simulate`/`serve`。
- `src/app_state.rs` / `mod.rs`：接线 A1/A2/A3。
- 合并验收：`cargo test`、既有 `simulation-corpus` 回归、e2e 全绿；更新 README。

## 依赖与顺序

- A1/A2 可并行；A3 依赖 A2 的 `ConfigStore`/pipeline 接口（先按 DESIGN §5 定契约，用 trait 解耦）。
- A4 只依赖 A3 的 HTTP 契约。
- A5 依赖 A3+A4 可运行。
- 冲突文件（`main.rs`/`mod.rs`/`app_state.rs`/`Cargo.toml`）由 A0 统一改；子 agent 需要时在 `docs/TASKS.md` 追加“待 A0 处理”条目。

## 完成状态（2026-09-14）

| 项 | 状态 | 说明 |
|---|---|---|
| A0 接口冻结 | ✅ | `src/bus.rs`（IncomingEvent/EventSink/PipelineOutcome）、`src/settings.rs`（AppConfig + ConfigStore 热载）、`config.example.json` |
| A1 Gateway | ✅ | `src/gateway/**`：白名单/drop、反向 WS（`0.0.0.0:9801`）、心跳、echo 动作回包、拒绝 `send_*` |
| A2 LLM+Pipeline | ✅ | `src/llm/**`：DeepSeek 客户端 + 规则快路径 + 权限（时段/预存）+ 内存重放 + who-whats |
| A3 HTTP API | ✅ | `src/api/**`：config(409)/board/display/messages/sim/members/gateway-status + 静态页 |
| A4 前端 | ✅ | `web/**`：display(5s 增量、不打断)/admin(config 面板+成员拉取)/sim(身份+偏移) |
| A5 测试 | ✅ | `tests/e2e/sim.mjs`（Playwright，4 用例 ALL PASS）、`package.json` |
| A0 装配 | ✅ | `main.rs run`：Gateway+Pipeline+API(`:21081`)+每日 19:00 成员调度 |
| 引擎缺陷修复 | ✅ | `AllocationEngine` 快照 `boxes`/`user_summaries` 排序确定化（修掉 flaky） |

**验证**：`cargo test` 46 passed；`node tests/e2e/sim.mjs` 4/4 PASS；`real-samples` 逐格回归 ALL PASS。

**运行**：
```powershell
cargo run -- run            # Gateway(0.0.0.0:9801) + API(127.0.0.1:21081) + 每日19:00成员拉取
# 展示 http://127.0.0.1:21081/  ·  管理 http://127.0.0.1:21081/admin  ·  模拟 http://127.0.0.1:21081/sim
node tests/e2e/sim.mjs      # 端到端测试
```

**遗留/后续**：真实 NapCat 接入后 `reply_enabled=false`（不回复）——需用户确认后再开；成员拉取依赖 NapCat 已连接；Cloudflare 远程展示（R2 发布 + Pages）尚未接线。

## 整理阶段小结（2026-09-14，已结清）

- ✅ **脱敏**：`pipeline.rs`/`onebot.rs` 测试改占位名；`agent-d-adversarial/*`、`real-xlsx/README.md` 真实昵称 → 占位；`viewer/` 已删除（合并进 `web/replay`）。
- ✅ **真实名单策略**：真实群成员名单只放 gitignored `data/members.seed.json`；入库仅 `data/members.example.json`（占位）。
- ✅ **D2 安全 + 去重**：`reply_enabled` 强制（`send_*` 仅当开启且白名单才放行，默认 false 绝不发送）；删除 `require_token`；昵称/时段/重放/`describe_event`/`truncate` 收敛到单一真源。
- ✅ **D3 死码/旧栈**：删除确认死代码；旧栈加 `#![allow(dead_code)]` + `//! DEPRECATED` 隔离；`cargo build` warning **142 → 0**；测试 **52 → 63**。
- ✅ **D5 文档**：`README.md` 重写；新增 `docs/README.md`、`docs/MODULES.md`；`DESIGN`/`POLICY`/`FUNCTIONAL` 同步（删 `require_token`、`reply_enabled` 已强制、viewer→`web/replay`、`/replay` 路由、测试数）。
- ✅ **D4/A4 前端**：`web/admin.html|js` 移除 `require_token` 控件。
- **校验**：`git grep` 真实昵称在跟踪文件 **0 命中**。
