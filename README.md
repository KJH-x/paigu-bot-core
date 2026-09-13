# paigu-bot-core

QQ 机器人拼团排谷系统后端，基于事件溯源架构，使用 LLM 解析自然语言消息，确定性引擎执行业务逻辑。

## 功能

- 自然语言排谷/撤销/修改 → LLM 解析 → 结构化事件
- 确定性排队与锁位引擎（优先级、包尾端盒、锁列、单领）
- 优惠分摊与赠品分配（满减、购物金、满赠，最大余数法）
- 账单结算（整数分 MoneyCents，无浮点误差）
- 反向 WebSocket 服务器接收 QQ 框架消息（port 3001）
- HTTP API 管理后台（port 8080）
- 快照发布至 Cloudflare R2 / 本地文件，前端静态页面实时展示
- CSV 导出（商品汇总、用户账单、下单辅助表）
- 全量事件重放与 StateDiff 差异追踪
- 离线仿真（JSONL 消息队列文件 + ParseCache 确定性重放）
- 审计链路追踪（DecisionTrace / AllocationTrace / ParseTrace）

## 技术栈

| 层面 | 技术 |
| --- | --- |
| 语言 | Rust 2021 |
| 异步运行时 | tokio |
| HTTP API | axum 0.7 |
| WebSocket | tokio-tungstenite 0.26 (反向WS服务器) |
| 数据库 | PostgreSQL + sqlx 0.8 |
| 对象存储 | Cloudflare R2 (aws-sdk-s3) |
| LLM | 抽象 trait，可接入任意 OpenAI 兼容客户端 |
| 日志 | tracing + tracing-subscriber |

## 快速开始

### 环境要求

- Rust 1.75+
- PostgreSQL 15+

### 配置

通过环境变量配置：

```env
# 数据库
DATABASE_URL=postgres://user:password@localhost/paigu_bot
DATABASE_MAX_CONNECTIONS=10

# LLM
LLM_API_BASE=https://api.openai.com/v1
LLM_API_KEY=sk-xxx
LLM_MODEL=gpt-4
LLM_MAX_TOKENS=2048
LLM_TEMPERATURE=0.0
LLM_CONFIDENCE_THRESHOLD=0.65

# R2 对象存储
R2_BUCKET=paigu-snapshots
R2_ENDPOINT=https://xxx.r2.cloudflarestorage.com
R2_ACCESS_KEY_ID=xxx
R2_SECRET_ACCESS_KEY=xxx

# HTTP API
HOST=0.0.0.0
PORT=8080

# WebSocket 服务器
WS_ENABLED=true
WS_HOST=0.0.0.0
WS_PORT=3001
WS_TOKEN=your-bearer-token

# 其他
DEFAULT_TIMEZONE=Asia/Shanghai
```

### 启动

```bash
# 数据库迁移（手动执行 SQL 文件）
psql -f migrations/001_initial_schema.sql

# 编译运行
cargo run --release
# HTTP API   → http://0.0.0.0:8080
# WebSocket  → ws://0.0.0.0:3001

# 运行测试
cargo test
```

### 离线模拟与本地聊天（无需 LLM / PostgreSQL）

```bash
# 确定性重放验证：读取商品表 + JSONL 消息队列，逐条解析→校验→重放，输出排结果与报告
cargo run -- simulate \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --queue        simulation-corpus/agent-a-normal/queue.jsonl \
  --out          simulation-corpus/agent-a-normal/out
# 产物: out/report.md（逐条状态+最终排结果+结算）、out/result.json、out/outcomes.jsonl

# 本地聊天界面模拟：浏览器输入话术，实时查看排结果（内存事件存储，实时重放）
cargo run -- serve \
  --round-config simulation-corpus/agent-a-normal/round_config.json \
  --port 8090
# 浏览器打开 http://127.0.0.1:8090
```

`simulate` 使用确定性规则解析器（`ParserMode::RuleOnly`），不调用 LLM，可完全复现。
话术覆盖：排/要/来/帮排、中文与阿拉伯数量、别名与错位语序、单领、代牌、包盒、包尾、撤销。
Policy：**购物金优先排（时间验证 valid_from），非购物金延后排**；排序键 `priority_level DESC → effective_at ASC → sequence ASC`。

验证结论与缺陷清单见 [simulation-corpus/VERIFICATION.md](./simulation-corpus/VERIFICATION.md)。
真实「事件重放」样本（变体/包尾/单领/调价）改造与逐项比对见 [simulation-corpus/real-samples/README.md](./simulation-corpus/real-samples/README.md)。
真实结果表（xlsx）解析、**逐格一致**验证与变体感知引擎见 [simulation-corpus/real-xlsx/README.md](./simulation-corpus/real-xlsx/README.md)。
真实群聊语料复现（两张 QQ 截图 + 商品价格归档 md）见 [simulation-corpus/real-chat/README.md](./simulation-corpus/real-chat/README.md)。
本次改造的操作-时间表见 [simulation-corpus/CHANGELOG.md](./simulation-corpus/CHANGELOG.md)。
重放步进查看器（表格 / **列视图** / Mermaid、双向高亮、样本切换）见 [viewer/README.md](./viewer/README.md)：`pwsh -File viewer\serve.ps1` → `http://127.0.0.1:8095/`。

## 项目结构

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
└── tests/        # 集成测试（9 个测试覆盖核心引擎）
```

## 核心架构

```text
QQ框架 ──WS──▶ ws_server (port 3001)
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

HTTP API (port 8080) ←─── 前端/管理后台
```

事件溯源：所有用户操作记录为不可变事件，最终状态由事件流确定性重放得到。LLM 仅负责自然语言到结构化数据的转换，不参与任何业务决策。

## WebSocket 消息协议

客户端（QQ 框架）连接 `ws://host:3001`，发送 JSON：

```json
{
  "type": "qq_message",
  "group_id": "123456",
  "user_id": "789",
  "nickname": "用户A",
  "message_id": "msg_001",
  "text": "排燐音吧唧2，蓝良单领1",
  "timestamp_ms": 1778241601000,
  "is_admin": false
}
```

服务器回复：

```json
{
  "message_id": null,
  "replyType": "text",
  "text": "已记录，当前版本 #128",
  "confirmToken": null
}
```

鉴权：在连接握手时携带 `Authorization: Bearer <token>` 头（`WS_TOKEN` 环境变量）。

## 文档

- [ARCHITECTURE.md](./ARCHITECTURE.md) - 完整系统架构与设计文档
- [REPLAY_SIMULATION_ADDENDUM.md](./REPLAY_SIMULATION_ADDENDUM.md) - 事件重放、图形化审计与模拟排谷补充设计
- [LOGIC_CHAINS.md](./LOGIC_CHAINS.md) - 全功能逻辑链条追踪（触发→事件流转→模块→结果）

## License

MIT
