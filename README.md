# paigu-bot-core

QQ 机器人拼团排谷系统后端，基于事件溯源架构，使用 LLM 解析自然语言消息，确定性引擎执行业务逻辑。

## 功能

- 自然语言排谷/撤销/修改 → LLM 解析 → 结构化事件
- 确定性排队与锁位引擎（优先级、包尾端盒、锁列）
- 优惠分摊与赠品分配
- 账单结算（整数分，无浮点误差）
- 快照发布至 Cloudflare R2，前端静态页面实时展示
- CSV 导出（商品、用户、订单维度）
- 全量事件重放与离线仿真
- 审计链路追踪

## 技术栈

| 层面 | 技术 |
|------|------|
| 语言 | Rust |
| 异步运行时 | tokio |
| HTTP | axum |
| 数据库 | PostgreSQL + sqlx |
| 对象存储 | Cloudflare R2 (S3) |
| LLM | 通用 trait，可接入任意兼容客户端 |

## 快速开始

### 环境要求

- Rust 1.75+
- PostgreSQL 15+
- LLM API 端点（兼容 OpenAI 接口格式）

### 配置

通过环境变量或 `.env` 文件：

```env
DATABASE_URL=postgres://user:password@localhost/paigu_bot
LLM_API_URL=https://your-llm-endpoint/v1
LLM_API_KEY=sk-xxx
LLM_MODEL=gpt-4
R2_ENDPOINT=https://xxx.r2.cloudflarestorage.com
R2_ACCESS_KEY=xxx
R2_SECRET_KEY=xxx
R2_BUCKET=paigu-snapshots
SERVER_PORT=3000
```

### 启动

```bash
# 数据库迁移
sqlx migrate run

# 编译运行
cargo run --release
```

## 项目结构

```
src/
├── domain/       # 核心数据类型（事件、排位、结算、快照）
├── engine/       # 业务引擎（重放、分配、结算、优惠）
├── parser/       # LLM 解析层（提示词、别名匹配、校验）
├── services/     # 编排层
├── repo/         # 数据访问层（PostgreSQL）
├── api/          # HTTP API 路由
├── inbound/      # QQ 消息接入
├── replay/       # 重放引擎
├── simulation/   # 离线仿真
├── audit/        # 审计追踪
├── storage/      # 快照持久化
└── publisher/    # 快照发布（R2 / 本地）
```

## 架构

事件溯源：所有用户操作记录为不可变事件，最终状态由事件流确定性重放得到。LLM 仅负责自然语言到结构化数据的转换，不参与任何业务决策。

详见 [ARCHITECTURE.md](./ARCHITECTURE.md)

## License

MIT
