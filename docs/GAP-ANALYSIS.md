# 差距分析与任务拆分（REQUIREMENTS 落地）

> 对照 [REQUIREMENTS.md](./REQUIREMENTS.md) C1–C7 与拼团/结算细节，盘点现状与缺口，并给出任务与文件所有权。

## 一、现状 vs 需求

| 需求 | 现状 | 缺口 | 结论 |
|---|---|---|---|
| C1 白名单成员实时+记录 | Gateway 仅按**群**白名单；消息只存**内存** `MessageRecord` | 缺**成员白名单**；缺**持久化消息日志**（JSONL）；Drop 不留痕 | **缺口大** |
| C2 重放+可改附加条件 | `ReplayEngine` 可重放事件；无「从消息日志重算」；无 overrides | 缺 `replay(messages, overrides)`；缺 override（优先队列/阶段/商品/折扣）；缺 diff | **缺口大** |
| C3 解耦+接口映射表 | 模块已有，但无完整接口文档 | 缺 `docs/INTERFACES.md` | **中** |
| C4 管理改数据+快照 | admin 仅改 config；无消息编辑；无快照导出 | 缺消息日志编辑（最高权限）；缺结构化快照包（config+日志+事件+结果+manifest） | **缺口大** |
| C5 全真模拟 WS client | `web/sim` 走 `POST /api/sim/message`（**旁路**，非 WS） | 需改为**WS 客户端**，真实连 `ws://…:9801`，共用管道 | **缺口大** |
| C6 脚本倍率模拟 | 无 | 缺脚本按 `--speed` 倍率经 WS 发消息 | **缺** |
| C7 录制+自动重放 | 无 | 缺 `record.jsonl` 录制与 `replay` 自动重放 | **缺** |
| 阶段模型 Phase 0/I/II/III/结算/锁定 | `RoundStatus` 枚举存在，但**无时间窗与权限矩阵** | 缺 `RoundPhase` 时间窗 + 权限矩阵 + 越权 Reject | **缺口大** |
| 调价（两模式） | 无 | 缺 PricingConfig（原价加减 / 直设调后价） | **缺** |
| 折扣份数/首 n 包/A-B 范围 | `settlement_engine` 有满减/固定/购物金，但**无份数、无首 n 包、无 A/B** | 缺份数与作用范围语义 | **缺口大** |
| 特典档位（多档/单包/叠加） | 仅 `GiftByThreshold` 单档 | 缺多档、单包独立、叠加、各自价格 | **缺口大** |
| 特典折价→减均 | 无 | 缺折价总额、减均范围选项、最大余数分摊 | **缺** |
| 下单表 + 自动搜索（两策略） | 无 | 缺 `OrderTable` + DFS/回溯（特典最多化/折扣最大化） | **缺** |
| 拖拽 UI | 无 | 缺结算/下单表页面与拖拽 | **缺** |

## 二、任务拆分（agents）

| 任务 | 范围（独占） | 交付 | 依赖 |
|---|---|---|---|
| **T1 基础设施** | `src/messages/**`(新)、`src/round/**`(新)、`src/settings.rs`、`src/gateway/**`、`src/llm/pipeline.rs` | ①`MessageStore`（JSONL 追加/读取/查询）+ 记录 Drop；②**成员白名单**（群×成员）；③`RoundPhase` 时间窗 + 权限矩阵 + 越权 Reject；④settings 增 `phases`/`whitelist_members`；⑤单测 | — |
| **T3 全真模拟** | `web/sim*`、`scripts/**`、`tests/**` | ①`web/sim` 改为 **WS 客户端**（连同一 gateway，发 OneBot 事件）；②`scripts/sim-run.mjs`（`--speed` 倍率经 WS 回放）；③录制 `record.jsonl` + `--replay` 自动重放；④e2e 更新 | — |
| **T7 接口文档** | `docs/INTERFACES.md` | 模块接口表 + 数据契约 + 需求→接口映射 + 解耦约束 | 需读全部模块（先起草） |
| **T4 结算引擎 v2** | `src/settlement/**`(新)、`src/settings.rs`(加 `settlement` 字段)、`src/domain/discount.rs` | 调价两模式；折扣份数/首 n 包/A-B 范围；特典多档/单包/叠加/各自价格；折价→减均（含/不含、按认购、最大余数）；可复算样例单测 | T1（settings 交接） |
| **T2 重放/快照/管理编辑** | `src/replay/session.rs`(新)、`src/snapshot_bundle/**`(新)、`src/api/**` | `replay(messages, overrides)` + diff；快照包导出/导入（zip/目录）；`/api/messages` 增改删（管理员）；`/api/replay/*` 实现 | T1 |
| **T5 下单表 planner** | `src/planner/**`(新) | `OrderTable` 模型；DFS/回溯搜索；特典最多化 / 折扣最大化 两策略；剪枝 | T4 |
| **T6 结算/下单表 UI** | `web/settlement*`(新)、`src/api/**`(结算路由) | 配置表单（调价/折扣/特典/减均）；试算与结果表；**拖拽 item→包**；两策略对比 | T4、T5 |
| **V 验收** | 只读 | 逐条 C1–C7 验收 + 回归 | 全部 |

## 三、波次与冲突规避
- **Wave A**：T1（`src/settings.rs` 独占）、T3（`web/sim*`/`scripts`/`tests`）、T7（`docs/`）。互不冲突。
- **Wave B**：T4（新增 `src/settlement/**`；在 T1 完成后给 `settings.rs` 加 `settlement` 字段）、T2（`src/api/**` + 新模块）。
- **Wave C**：T5（`src/planner/**`）、T6（`web/settlement*` + `src/api/**` 结算路由；与 T5 分文件）。
- **Wave D**：V。
- 公共文件（`main.rs`、各 `mod.rs`、`Cargo.toml`）由 **A0** 统一装配。

## 四、A0 冻结接口（先行）
为避免并行冲突，A0 先定义并冻结：
- `src/messages/mod.rs`：`MessageRecord` / `MessageStore` trait（`append/read_all/query/update/delete`）。
- `src/round/mod.rs`：`RoundPhase`（枚举+时间窗）/ `PhasePolicy` / `can_claim/can_cancel`。
- `src/settlement/mod.rs`：`SettlementConfig`（Pricing/Discount/Gift/ReduceAverage）占位类型。
- `src/snapshot_bundle/mod.rs`：`SnapshotBundle` 结构。
各 agent 在其上实现。

## 五、验收口径
- C1：白名单外成员消息被 Drop 且落日志；白名单成员落库可查。
- C2：同日志 + 不同 overrides → 结果差异可复现、可 diff。
- C3：`INTERFACES.md` 覆盖所有模块与 C1–C7 映射。
- C4：WebUI 改一条申购消息后重算，快照包可导出并再导入复算一致。
- C5：WS client 发的消息在服务端日志与展示页可见（走同一管道）。
- C6：`--speed 10` 的脚本回放结果与实时一致（仅时间压缩）。
- C7：录制→重放结果一致。
- 阶段/结算：权限矩阵单测 + 结算可复算样例（含 501 元三档特典）。
