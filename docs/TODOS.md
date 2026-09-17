# 工程待办（TODOS）

> 每条：ID / 优先级(P0>P1>P2>P3) / 状态 / 依赖 / 验收。
> 状态：`todo` 未开始 · `doing` 进行中 · `blocked` 被依赖阻塞（注明 D-xx）· `done` 完成。
> 领取任务前先读 [AGENT-RULES.md](./AGENT-RULES.md) 与 [MODULES.md](./MODULES.md)（文件所有权）。
> 决策依赖见 [DECISIONS.md](./DECISIONS.md)（标注 D-xx）。

## P0（阻塞验收或安全）

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-01 | todo | **结算 UI 传入基础单价**：`web/settlement` 从排位/配置生成 `OrderTable` 时单价为 0，需接入商品标价（A-4 提供后）或表内编辑持久化 | A-4 | 试算金额非 0 且可复算 |
| T-02 | todo | **`MessageStore` 共享单例接线**：Gateway 与 Pipeline 目前各自 `from_env` 构造实例写同一文件（靠进程内锁串行），改为 `main.rs` 注入共享 `Arc<dyn MessageStore>` | — | 并发写不交错；单测 |
| T-03 | todo | **真实昵称零残留长期守护**：CI/脚本扫描跟踪文件（69 人名单 + 4 优先昵称），命中即失败 | — | 扫描脚本入库并在回归中运行 |
| T-04 | todo | **`docs/FUNCTIONAL.md` 与实现对齐守护**：FUNCTIONAL 是评审主文档；实现变更需同步（当前引用 `pipeline.rs:188` 等行号易漂移，改为引用函数名） | — | 审阅确认 |

## P1（功能补齐）

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-05 | todo | **阶段权限接入实时链路**：`RoundPhase`/`ItemClass` 已实现并单测，但实时 Pipeline 的**商品分类（A/B）来源**需在 LLM/规则抽取中回填（当前 `ItemConfig.class` 仅配置） | — | 阶段越权 Reject 的 e2e 用例 |
| T-06 | todo | **消息编辑 API 的写回持久化**：`/api/messages` 增改删已实现（replace_all），确认写回 JSONL 并触发重算（含 `?recompute`） | — | 改一条后 `/api/display` 变化 |
| T-07 | todo | **快照导出/导入前端入口**：`POST /api/snapshot/export|import` 已实现，`web/admin` 缺按钮与下载 | — | admin 页可导出/导入 |
| T-08 | todo | **`/api/replay` diff 展示**：后端已返回 diff；`web/replay` 增「对比实时态」视图 | — | diff 可视化 |
| T-09 | todo | **成员 user_id 映射**：NapCat 拉取后把 `user_id` 写入成员缓存，权限/幂等用 user_id（现占位名单 user_id=null） | B-2 | 拉取后映射非空 |
| T-10 | todo | **每日 19:00 成员拉取验证**：需 NapCat 在线；失败告警（可接 Bark） | B-2 | 拉取成功且缓存更新 |
| T-11 | todo | **`simulate` 走新栈统一管道**（D-4 若选 A） | D-4 | 同日志同结果 |
| T-12 | todo | **`MessageStore` 细粒度方法**（C-5 若选 B） | C-5 | 增改删不再全量重写 |

## P2（增强 / 清理）

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-13 | todo | **结算配置入库**：`settlement` 字段样例加入 `config.example.json`（含折扣/特典/减均样例） | — | 样例可加载 |
| T-14 | todo | **`IncomingEvent.raw` 接入事件溯源或删除**（C-3） | C-3 | 无 dead_code 告警 |
| T-15 | todo | **入站有界队列/背压**（DESIGN §7 提到的 mpsc，当前 Gateway 直接 spawn 调 sink） | — | 压测不丢消息 |
| T-16 | todo | **管理员斜杠命令落地**（D-1） | D-1 | 命令生效并重算 |
| T-17 | todo | **`Modify` 改单实现**（D-2） | D-2 | 改数量后重算 |
| T-18 | todo | **`DiscountRule`（旧 domain）与新 `SettlementConfig` 关系定稿**（避免两套折扣模型） | A-1 | 单一模型 |
| T-19 | todo | **拆大文件**：`llm/pipeline.rs`(1000+)、`simulation/verifier.rs`(700+) 拆子模块 | — | 行数/职责清晰 |
| T-20 | todo | **补单测**：`domain/**`、`engine/replay`、`api/board_routes` | — | 覆盖率提升 |

## 已完成（归档）

| ID | 任务 | 提交 |
|---|---|---|
| ✅ | 消息日志/成员白名单/阶段权限（T1） | `510c54a` |
| ✅ | WS 客户端模拟/脚本倍率/录制重放（T3） | `510c54a` |
| ✅ | 接口映射表（T7） | `510c54a` |
| ✅ | 结算引擎 v2（T4） | `7a67caa` |
| ✅ | 重放覆盖/快照包/消息编辑 API（T2） | `7a67caa` |
| ✅ | 下单表 planner（T5） | `de06d22` |
| ✅ | 结算/下单表 UI（T6） | `de06d22` |
| ✅ | 死码清理/旧栈隔离（D3） | `300837a` |
| ✅ | 文档重写与脱敏（D5） | `300837a` |
| ✅ | reply_enabled 强制 / require_token 移除（D2） | `01eb038` |
