# paigu-bot-core

QQ 机器人拼团排谷系统：**本地做数据处理，远程做展示**。本机接收 QQ 群消息，经 LLM 清理与抽取、确定性引擎排谷计算；排位快照与回放将来发布到 Cloudflare（R2 + Pages）供成员查看（远程发布尚未接线）。

> 定位：**本地处理 + 远程展示**。本地（本机 `部署主机`）负责接入、解析、排谷；远程只负责只读展示。
> 安全红线：`reply_enabled` 默认 `false`，**绝不主动发消息给真实群**（只读拉取成员名单允许）。
> 必读文档：**[docs/POLICY.md](./docs/POLICY.md)**（业务政策）· **[docs/DESIGN.md](./docs/DESIGN.md)**（程序路线）· **[docs/TASKS.md](./docs/TASKS.md)**（任务拆分）· **[docs/AGENT-RULES.md](./docs/AGENT-RULES.md)**（协作规则）· **[docs/FUNCTIONAL.md](./docs/FUNCTIONAL.md)**（功能描述）· **[docs/MODULES.md](./docs/MODULES.md)**（模块契约与文件所有权）。
>
> **Warning:** This project is under active development — large, unexpected diffs may appear at any time as message merging (same-sender coalescing) evolves. Contribute at your own risk.

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
     / /index /round /display /settlement /replay /members /settings 与 /web/*
                           ▼
   浏览器展示页（local 数据源，5s 增量轮询）
   ┈┈┈┈（尚未接线）┈┈┈▶ Cloudflare R2(快照/回放) + Pages(静态展示)
```

- 单进程：Gateway + Pipeline + HTTP API 同进程运行；`run` 模式**不连接 PostgreSQL**。
- 确定性优先：LLM 只做自然语言 → 结构化；排序/分配/结算由确定性引擎完成，任何 LLM 输出都经校验层。
- 事件溯源：所有操作记录为不可变事件，最终状态由事件流确定性重放得到。
- 消息/原自动理解及人工审计写入私有日志，启动先重放恢复，再接收新消息；坏/半截日志拒绝启动，不回退猜测。排位为可重建派生状态，私有配置、轮次、成员缓存和订单草稿不入库。

## 运行

```bash
# 新栈（默认）：Gateway + Pipeline + HTTP API + 每日 19:00 成员拉取
cargo run -- run
#   反向 WS   → ws://0.0.0.0:9801（NapCat 主动连入；白名单群 123456789；默认不回复）
#   HTTP API  → http://127.0.0.1:21081
#   排谷 /display  总览 /index  轮次 /round  成员 /members
#   结算 /settlement  复盘 /replay  系统 /settings（/admin兼容转总览，/sim已移除）

# 端到端测试（Playwright，脚本自带 cargo build + 临时服务）
node tests/e2e/sim.mjs
```

公开协作无需NapCat、真实名单或模型：`npm run check` 提供隔离核心/脚本/文档/离线CLI与隐私门禁，`npm run check:full` 再执行完整桌面。私有语料显式选择，未提供显示NOT RUN而不是通过；依赖、隔离路径及可选参数见 [开发检查](./docs/DEVELOPMENT.md)。

离线验证（与当前确定性会话共用执行；夹具版本与兼容元数据说明见 [开发检查](./docs/DEVELOPMENT.md)）：

```bash
# 确定性重放：商品表 + JSONL 消息队列 → 排结果与报告
cargo run -- simulate \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --queue        simulation-corpus/agent-a-normal/queue.jsonl \
  --out          "C:/private-fixtures-output/out"

# 语料夹具与逐格验证（先经 simulate 生成 out/，再校验；详见对应 README）
python tests/read-only-corpus.py   # 只读源表，产物仅写临时目录，不改语料资产
node   simulation-corpus/real-samples/verify-samples.mjs simulation-corpus/real-samples

# 隐私/密钥扫描（跟踪文件命中真实昵称/密钥/内网 IP/data·config·xlsx 入库即失败）
npm run privacy

# 隐私守护自测：临时Git仓库、合成数据，无需NapCat或私有名单
npm run test:privacy
```

> `run` / `serve` 均走新栈；任何未识别的子命令按 `run` 启动。旧栈与 PostgreSQL 已在 **C-4** 删除（不再有 `DATABASE_URL` 回退路径）。

## 目录结构

```text
src/
├── main.rs          # 入口分派：run / serve → 新栈；simulate → 离线重放；其余按 run
├── bus.rs           # 冻结接口：IncomingEvent / EventSink / PipelineOutcome
├── settings/        # AppConfig + ConfigStore（revision + 可恢复热载）
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
web/                 # 七工作区静态页；admin兼容跳转，无sim页
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
- 校验：`npm run privacy` 同时扫描Git暂存索引与工作副本，防止已暂存的敏感内容被干净/删除的工作文件掩盖。已知私有源涵盖群名片及清洗称呼、CN/别名、自定义成员缓存、拆分轮次及数字身份；坏隐私源会失败，不打印匹配内容或自定义私有路径。vendor只豁免词典姓名匹配，密钥/IP不豁免（扫描器自身同样检查）。禁止跟踪 `data/**`（占位例外）、`config/**`、工作簿。
- `npm run test:privacy` 提供21类本地隔离回归，不连接真实接收器或模型。只豁免已知占位值，不能靠修改示例把私有标识加入豁免。无私有源的公开checkout仍可检查通用密钥/IP/禁止路径，但无法识别所有未知真实称呼；检测不等于完整匿名化或历史清洗，公开前仍须审查数据来源及历史。不要把私有备份/完整消息日志上传到Issue或PR。

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
