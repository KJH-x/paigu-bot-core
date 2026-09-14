# paigu-bot-core · 旧版 README（历史归档）

> **本文件是 2026-09-14 重构前的 README 存档，已被根目录 [README.md](../../README.md) 取代。**
> 其中关于反向 WS 端口 `3001`、HTTP API 端口 `8080`、以及旧 `{ "type": "qq_message" }` 入站协议的内容
> 属于**已废弃的旧栈**（需 PostgreSQL，`cargo run -- <未识别子命令>` 才会进入），已从新文档中删除，
> 此处不再保留，以免误导。现行架构与运行方式见 [README.md](../../README.md) 与 [../DESIGN.md](../DESIGN.md)。

## 定位

QQ 机器人拼团排谷系统后端，基于事件溯源架构，使用 LLM 解析自然语言消息，确定性引擎执行业务逻辑。

## 功能（历史）

- 自然语言排谷/撤销/修改 → LLM 解析 → 结构化事件
- 确定性排队与锁位引擎（优先级、包尾端盒、锁列、单领）
- 优惠分摊与赠品分配（满减、购物金、满赠，最大余数法）
- 账单结算（整数分 MoneyCents，无浮点误差）
- 快照发布至 Cloudflare R2 / 本地文件，前端静态页面实时展示
- CSV 导出（商品汇总、用户账单、下单辅助表）
- 全量事件重放与 StateDiff 差异追踪
- 离线仿真（JSONL 消息队列文件 + ParseCache 确定性重放）
- 审计链路追踪（DecisionTrace / AllocationTrace / ParseTrace）

## 技术栈（历史）

| 层面 | 技术 |
| --- | --- |
| 语言 | Rust 2021 |
| 异步运行时 | tokio |
| HTTP API | axum 0.7 |
| WebSocket | tokio-tungstenite 0.26 |
| 数据库 | PostgreSQL + sqlx 0.8（仅旧栈） |
| 对象存储 | Cloudflare R2 (aws-sdk-s3) |
| LLM | 抽象 trait，可接入任意 OpenAI 兼容客户端 |
| 日志 | tracing + tracing-subscriber |

## 旧栈项目结构

```text
src/
├── domain/       # 核心数据类型（IDs, Money, Round, Item, Claim, Event, Allocation, Settlement, Snapshot, Discount, Gift）
├── engine/       # 业务引擎（EventStore, Replay, Allocation, Settlement, Discount, Gift, Priority, Policy）
├── parser/       # LLM 解析层（LlmClient, Prompt, ParsedEvent, AliasMatch, Validation, ParseCache）
├── services/     # 编排层（Message, Round, Admin, Claim, Cancel, Snapshot, Settlement, Export）
├── repo/         # 数据访问层 trait 定义（PostgreSQL impl）
├── api/          # HTTP API 路由（Admin, Public, Webhook, Replay, Simulation）
├── inbound/      # QQ 消息接入（IncomingQqMessage, Intake, CommandRouter）
├── ws/           # 反向 WebSocket 服务器（接收 QQ 框架消息）
├── replay/       # 重放引擎（ReplayEngine, StateDiff, TimelineSnapshot, ReplayReport）
├── simulation/   # 离线仿真（QueueFile, SimulationRunner, Fixtures）
├── audit/        # 审计追踪（DecisionTrace, AllocationTrace, ParseTrace, RuleTrace）
├── storage/      # Timeline 持久化（TimelineStore）
├── publisher/    # 快照发布（R2Publisher, LocalPublisher）
└── tests/        # 集成测试
```

## 旧栈核心架构（概念）

```text
QQ框架 ──WS──▶ ws_server（旧栈）
                    │
                    ▼
          IncomingQqMessage
                    │
          ┌─────────┴──────────┐
          │  command_router    │
          │  classify_message  │
          └─────────┬──────────┘
                    │
          ┌─────────┴──────────┐
          │  MessageService    │
          │  (幂等校验+状态检查)  │
          └─────────┬──────────┘
                    │
     ┌──────────────┼──────────────┐
     ▼              ▼              ▼
  Parser        EventStore      Replay
  (LLM解析)     (事件写入)      (事件重放)
                    │
     ┌──────────────┼──────────────┐
     ▼              ▼              ▼
  Allocation     Settlement     Snapshot
  Engine         Engine         Publisher
  (排队分配)     (结算分摊)     (R2发布)

旧栈 HTTP API ←─── 前端/管理后台
```

事件溯源：所有用户操作记录为不可变事件，最终状态由事件流确定性重放得到。LLM 仅负责自然语言到结构化数据的转换，不参与任何业务决策。

## 离线模拟与本地聊天（保留）

```bash
# 确定性重放验证
cargo run -- simulate \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --queue        simulation-corpus/agent-a-normal/queue.jsonl \
  --out          simulation-corpus/agent-a-normal/out

# 本地聊天界面模拟
cargo run -- serve \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --port 8090
```

## 历史设计文档（gitignored，勿引用）

- `ARCHITECTURE.md` - 完整系统架构与设计文档
- `REPLAY_SIMULATION_ADDENDUM.md` - 事件重放、图形化审计与模拟排谷补充设计
- `LOGIC_CHAINS.md` - 全功能逻辑链条追踪

## License

MIT
