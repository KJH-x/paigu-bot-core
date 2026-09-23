# paigu-bot-core

QQ 机器人拼团排谷系统：**本地做数据处理，远程做展示**。本机接收 QQ 群消息，经 LLM 清理与抽取、确定性引擎排谷计算；排位快照与回放发布到 Cloudflare（R2 + Pages）供成员查看。

> 定位：**本地处理 + 远程展示**。本地（本机 `部署主机`）负责接入、解析、排谷；远程只负责只读展示。
> 安全红线：`reply_enabled` 默认 `false`，**绝不主动发消息给真实群**（只读拉取成员名单允许）。
> 必读文档：**[docs/POLICY.md](./docs/POLICY.md)**（业务政策）· **[docs/DESIGN.md](./docs/DESIGN.md)**（程序路线）· **[docs/TASKS.md](./docs/TASKS.md)**（任务拆分）· **[docs/AGENT-RULES.md](./docs/AGENT-RULES.md)**（协作规则）· **[docs/FUNCTIONAL.md](./docs/FUNCTIONAL.md)**（功能描述）· **[docs/MODULES.md](./docs/MODULES.md)**（模块契约与文件所有权）。

## 运行时架构

```text
 NapCatQQ ──反向 WS──▶ Gateway 0.0.0.0:9801
                           │ 白名单/drop · 心跳 · 只读动作回包（禁止 send_*）
                           ▼
                    IncomingEvent（src/bus.rs）
                           ▼
   Pipeline：幂等 → 规则快路径 → LLM 清理/抽取 → 校验 → 权限(时段/预存)
                           ▼
        内存事件列表 → rebuild(Replay + Allocation) → AllocationSnapshot
                           ▼
   axum HTTP API 127.0.0.1:21081
     /api/config /api/board /api/display /api/messages /api/sim/* /api/members /api/gateway/status
     / /admin /sim /replay 与 /web/* 静态页（fallback 目录 web/）
                           ▼
   浏览器展示页（local 数据源，5s 增量轮询）
   ┈┈┈┈（尚未接线）┈┈┈▶ Cloudflare R2(快照/回放) + Pages(静态展示)
```

- 单进程：Gateway + Pipeline + HTTP API 同进程运行；`run` 模式**不连接 PostgreSQL**。
- 确定性优先：LLM 只做自然语言 → 结构化；排序/分配/结算由确定性引擎完成，任何 LLM 输出都经校验层。
- 事件溯源：所有操作记录为不可变事件，最终状态由事件流确定性重放得到。
- 当前为内存态：事件/快照不持久化，进程重启即丢失（`data/` 仅存成员缓存）。

## 运行

```bash
# 新栈（默认）：Gateway + Pipeline + HTTP API + 每日 19:00 成员拉取
cargo run -- run
#   反向 WS   → ws://0.0.0.0:9801（NapCat 主动连入；白名单群 123456789；默认不回复）
#   HTTP API  → http://127.0.0.1:21081
#   展示 /  管理 /admin  模拟 /sim  重放 /replay

# 端到端测试（Playwright，脚本自带 cargo build + 临时服务）
node tests/e2e/sim.mjs
```

离线验证（确定性重放；旧 verifier 暂留以支撑语料回归，见 T-24）：

```bash
# 确定性重放：商品表 + JSONL 消息队列 → 排结果与报告
cargo run -- simulate \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --queue        simulation-corpus/agent-a-normal/queue.jsonl \
  --out          simulation-corpus/agent-a-normal/out

# 语料夹具与逐格验证（先经 simulate 生成 out/，再校验；详见对应 README）
python simulation-corpus/real-xlsx/build_fixtures.py
node   simulation-corpus/real-samples/verify-samples.mjs simulation-corpus/real-samples

# 隐私/密钥扫描（跟踪文件命中真实昵称/密钥/内网 IP/data·config·xlsx 入库即失败）
npm run privacy
```

> `run` / `serve` 均走新栈；任何未识别的子命令按 `run` 启动。旧栈与 PostgreSQL 已在 **C-4** 删除（不再有 `DATABASE_URL` 回退路径）。

## 目录结构

```text
src/
├── main.rs          # 入口分派：run / serve → 新栈；simulate → 离线重放；其余按 run
├── bus.rs           # 冻结接口：IncomingEvent / EventSink / PipelineOutcome
├── settings.rs      # AppConfig + ConfigStore（revision + 热载）
├── error.rs         # thiserror 错误类型（AppError + 各领域错误）
├── gateway/         # OneBot 反向 WS：路由(白名单/drop)、心跳、echo 动作回包、只读动作
├── llm/             # DeepSeek 客户端 + 排谷流水线（规则快路径 + LLM 清理/抽取 + 回退）
├── api/             # axum 路由：config/board/display/messages/replay/settlement/sim/members + 静态页
├── domain/          # 领域模型：事件/快照/分配/结算/商品/用户
├── engine/          # 确定性分配引擎 + 事件存储 + 重放
├── parser/          # 规则解析 + 校验 + 归一化 + 别名匹配
├── planner/         # 下单表 planner（GiftMax / DiscountMax）
├── replay/          # 逐步重放引擎 + 会话 + state diff
├── round/           # 拼团阶段模型（时间窗 + 权限矩阵）
├── settlement/      # 结算引擎 v2（减均 / 成团 / 特典）
├── simulation/      # 离线 simulate verifier + 队列
├── snapshot_bundle/ # 单一 JSON 快照导出/导入
├── messages/        # 消息日志 MessageLog（JSONL + 原始事件）
└── tests/           # Rust 重放辅助测试
web/                 # 静态前端：display / admin / sim / replay / settlement + common.js
docs/                # 现行文档（入口 docs/README.md）+ archive/
tests/e2e/           # Node .mjs Playwright 端到端测试
scripts/             # 模拟/录制脚本 + 隐私扫描 privacy-scan.mjs
config.example.json  # 配置模板（首次运行据此生成 config/app.json）
data/members.example.json  # 占位成员（真实名单不入库）
simulation-corpus/   # 回归语料（只读资产，不得修改）
```

## 配置与热载

- 配置文件：`config/app.json`（gitignored）。首次运行若不存在，会由内嵌的 `config.example.json` 生成。
- 存储：`ConfigStore`（`tokio::sync::RwLock<AppConfig>` + `revision`）。
  - `PUT /api/config`：乐观并发，`revision` 不匹配 → `409 stale_revision`；成功则 `revision + 1` 并落盘。
  - 文件变更：`notify` 监听 + 300ms 去抖后自动热载（`revision` 单调，不回退）。
- 主要字段：`gateway.{bind,whitelist_groups,heartbeat_secs,reply_enabled,allowed_actions}`、`llm.*`、`round.{round_id,title,group_id,priority_users,priority_window,items}`、`display.*`、`members.*`。
- 环境变量：`PAIGU_CONFIG_PATH`（默认 `config/app.json`）、`PAIGU_HTTP_PORT`（默认 `21081`）、`PAIGU_WEB_DIR`（默认 `web`）、`PAIGU_MEMBERS_SEED_PATH` / `PAIGU_MEMBERS_EXAMPLE_PATH`、`DEEPSEEK_API_KEY`（由 `llm.api_key_env` 指定名）。

## 脱敏策略

- **真实群成员名单只放 gitignored 的 `data/members.seed.json`**；入库仅 `data/members.example.json`（占位名 `成员01`…`成员05`）。
- `config/app.json`、`data/**`、`*.xlsx`、`sample*.json`、`simulation-corpus/real-*/` 均被 `.gitignore` 忽略，保持忽略状态。
- 文档与语料使用占位名（`用户A` / `成员01` 等）；语料内部原版 `*.md` 被忽略，入库为 `*.public.md` 脱敏版。
- 密钥不入代码：LLM key 仅经 `api_key_env`（`DEEPSEEK_API_KEY`）读取。
- 校验：`npm run privacy`（`scripts/privacy-scan.mjs`）扫描全部 `git ls-files` 跟踪文件，命中真实昵称/密钥/内网 IP，或 `data/**`、`config/**`、`*.xlsx` 被跟踪即失败；另可用 `git grep` 复核真实昵称应为 **0 命中**。

## 文档索引

**现行（必读）** — 见 [docs/README.md](./docs/README.md)
- [docs/POLICY.md](./docs/POLICY.md) - 业务政策（接入/白名单/清洗/LLM 流水线/权限时段/展示/成员/热载）
- [docs/DESIGN.md](./docs/DESIGN.md) - 程序设计路线（架构/模块/接口/配置/路由/部署）
- [docs/TASKS.md](./docs/TASKS.md) - 任务拆分与文件所有权
- [docs/AGENT-RULES.md](./docs/AGENT-RULES.md) - 子 agent 协作规则
- [docs/FUNCTIONAL.md](./docs/FUNCTIONAL.md) - 功能描述（面向评审，与实现一致）
- [docs/MODULES.md](./docs/MODULES.md) - 模块契约与文件所有权
- [docs/archive/README-legacy.md](./docs/archive/README-legacy.md) - 旧版 README 归档

**语料与前端**
- [web/README.md](./web/README.md) - 前端说明（display / admin / sim / replay）
- [simulation-corpus/VERIFICATION.public.md](./simulation-corpus/VERIFICATION.public.md) - 确定性重放验证报告（脱敏版）
- [simulation-corpus/CHANGELOG.public.md](./simulation-corpus/CHANGELOG.public.md) - 操作-时间表（脱敏版）
- [simulation-corpus/real-chat/README.public.md](./simulation-corpus/real-chat/README.public.md) - 真实群聊语料复现（脱敏版）
- [simulation-corpus/real-samples/README.md](./simulation-corpus/real-samples/README.md) - 事件重放样本
- [simulation-corpus/real-xlsx/README.md](./simulation-corpus/real-xlsx/README.md) - 真实结果表解析与逐格验证

**历史（gitignored，勿引用）**
- `ARCHITECTURE.md`、`LOGIC_CHAINS.md`、`REPLAY_SIMULATION_ADDENDUM.md`

## License

MIT
