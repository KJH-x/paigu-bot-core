# 工程待办（TODOS）

> 每条：ID / 优先级(P0>P1>P2) / 状态 / 依赖 / 验收。
> 状态：`todo` · `doing` · `blocked`（注明 D-xx）· `done`。决策依据见 [DECISIONS.md](./DECISIONS.md)。
> 领任务前先读 [AGENT-RULES.md](./AGENT-RULES.md) 与 [MODULES.md](./MODULES.md)（文件所有权）。

## P0（核心正确性 / 安全）

> **Wave 1 已交付**（2026-09-17）：T-01 减均重构 ✅ · T-02 成团/特典认购 ✅ · T-03 标价表入商品目录（后端 ✅／UI 待办）· 新夹具 `月行水上-更新` ✅（≈ Sheet2 参考价逐单一致）。口径见 [DECISIONS.md](./DECISIONS.md) §E。

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-01 | done | **减均重构（A-1）**：`B=Σ各包实付价`、`C=无折扣商品总价`、`D=C−B+G`，按各商品标价加权分摊 `D`；**2 位小数 + 四舍五入 + 残差校正**；校验 `Σ final_i + G = B` | A-1 | ✅ 单测含校验式/边界/逐件精度 |
| T-02 | done | **成团/特典认购（A-2/DECISIONS §E）**：`成团数=⌊认购件数/每盒件数⌋`，不足整盒不成团（无包尾）；`granted=min(claims,P)`，`G=Σgranted×特典价` | A-2 | ✅ 月行水上-更新 G=¥144 |
| T-03 | doing | **标价表入商品目录（A-4）**：`round.items[]`+`variants[]` 增标价/盒件数/件数并透传；结算/planner 改从目录取标价（`to_unit_prices`） | A-4 | ✅ 后端与夹具；⏳「添加商品」UI 手工确认待办 |
| T-04 | todo | **`reply_enabled` 热切换开关**（B-1）：运行时可开/关（不重启），管理员权限；默认关闭 | B-1 | 关闭时 `send_*` 必被拦截 |
| T-05 | todo | **`MessageStore` 共享单例注入**（原 T-02）：`main.rs` 注入 `Arc<dyn MessageStore>`，Gateway/Pipeline 共用 | — | 并发写不交错 |

## P1（功能补齐）

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-06 | todo | **单一 JSON 快照（C-2）**：`{version, computed_at, messages, events, result_cache}` 取代目录式 bundle | C-2 | 单文件导出/导入往返一致 |
| T-07 | todo | **原始事件落盘（C-3）**：`IncomingEvent.raw` 接入事件溯源并持久化 | C-3 | 事件可回读重放 |
| T-08 | todo | **`MessageStore::query/update/delete`（C-5）**：细粒度方法，替代 replace_all 全量重写 | C-5 | 编辑不再全量重写 |
| T-09 | todo | **管理员命令执行 + 热开关（D-1）**：`/开团 /结团 /加商品 /锁位 /修正 /设置优惠 /导出` 落地，开关控制 | D-1 | 命令生效并触发重算 |
| T-10 | todo | **`Modify` 改单（D-2）**：成员改自己、管理员改任意人 | D-2 | 改数量后重算 + 权限用例 |
| T-11 | todo | **`simulate`/`serve` 统一到新栈（D-4）**：并入 Gateway→Pipeline 管道 | D-4 | 同输入同结果 |
| T-12 | todo | **删除旧栈（C-4）**：按 DECISIONS 清单删除（`config/app_state/repo/services/ws/publisher/旧 api/inbound/chat_server`）+ 清理 `Cargo.toml` 旧依赖 | C-4🟡 | `cargo build/test` 全绿 |

## P2（增强 / 清理）

| ID | 状态 | 任务 | 依赖 | 验收 |
|---|---|---|---|---|
| T-13 | todo | **阶段权限接入实时链路**：商品 A/B 分类来源回填（现仅配置） | A-5 | 越权 Reject 的 e2e |
| T-14 | todo | **快照导出/导入前端入口**（`web/admin` 按钮 + 下载） | T-06 | admin 可操作 |
| T-15 | todo | **`/api/replay` diff 可视化**（`web/replay`） | — | diff 可见 |
| T-16 | todo | **成员 user_id 映射**（NapCat 拉取写入缓存） | B-2 | 映射非空 |
| T-17 | todo | **每日 19:00 拉取验证 + 失败告警** | B-2 | 缓存更新/告警 |
| T-18 | todo | **真实昵称零残留守护脚本**（纳入回归） | — | 命中即失败 |
| T-19 | todo | **`FUNCTIONAL.md` 引用改函数名**（现行号易漂移） | — | 不再依赖行号 |
| T-20 | todo | **结算配置样例入 `config.example.json`** | — | 样例可加载 |
| T-21 | todo | **入站有界队列/背压**（DESIGN §7 的 mpsc） | — | 压测不丢消息 |
| T-22 | todo | **拆大文件**：`llm/pipeline.rs`(1000+)、`simulation/verifier.rs`(700+) | T-11 | 职责清晰 |
| T-23 | todo | **补单测**：`domain/**`、`engine/replay`、`api/board_routes` | — | 覆盖率提升 |

## 已完成（归档）

| 任务 | 提交 |
|---|---|
| 消息日志 / 成员白名单 / 阶段模型（T1） | `510c54a` |
| WS 客户端模拟 / 脚本倍率 / 录制重放（T3） | `510c54a` |
| 接口映射表（T7） | `510c54a` |
| 结算引擎 v2（T4） | `7a67caa` |
| 重放覆盖 / 快照包 / 消息编辑 API（T2） | `7a67caa` |
| 下单表 planner（T5） | `de06d22` |
| 结算 / 下单表 UI（T6） | `de06d22` |
| 死码清理 / 旧栈隔离 | `300837a` |
| 文档重写与脱敏 | `300837a` |
| reply_enabled 强制 / require_token 移除 | `01eb038` |
| 文档接手入口 + DECISIONS + TODOS | `fb99c9e` |
