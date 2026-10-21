# SPEC · 唯一事件处理链（统一处理 + 阶段锁 + computed 缓存）（2026-10-13）

> 状态：**2026-10-13历史方案，2026-10-21重新审计；不得逐字执行**。共同写边界/持久理解/原位修订等已由后续专项实现；旧默认值、游标、原文编辑和锁定草案有冲突，见文末。有效剩余工程项以 [后继计划](./PLAN-engineering-followup-2026-10-19.md) 和现行DECISIONS §H/INTERFACES为准。
> 目标：**只有唯一事件处理**——`传入消息 → 落盘储存 → 即时重算(live) 或 分步重算检查(复盘)`。

## 1. 目标数据流

```
NC ─原始消息─► ① 预处理并落盘(data/messages/*.jsonl, append-only)
                ├─ 伪造解包(gate_nc, 仅测试群)
                ├─ 具名代排 `代X排:` → 切段为多条记录
                └─ 匿名代排 `X(代)` → 合成身份 `发送者(代)`（幂等）
              ② LLM 处理（仅"需要 LLM 的组"）→ ③ ParseOverride 落盘(data/events/*.jsonl)
              ④ 唯一处理器 compute(日志) → 动作记录(事件) + 看板
                  ├─ live：即时重算（增量）
                  └─ 复盘：分步重算检查（按消息组）
              ⑤ 落盘 computed 缓存(data/computed/<round>.json) → 读取层(Live/复盘/前端只读)
```
- **前端只读取与展示，不计算**；按当前缓存增量拉取（消息按 `seq`、看板按 `version`）。
- **合并默认开**（`merge.enabled=true`，`MergeSettings::default`）。

## 2. 阶段锁（取代 `State.locked`）

**锁 = 生效阶段 ∈ {Settling, Locked}**。阶段来源 = 时间窗 `phase_at(ts)`，可被**手动阶段覆盖事件**"快进"。

```rust
// src/domain/event.rs
pub enum DomainEvent { /* … */ RoundPhaseSet(RoundPhaseSet) }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundPhaseSet { pub phase: RoundPhase, pub set_by: UserId }
// event_type_str → "round_phase_set"
```
```rust
// src/round/mod.rs：Settling/Locked 均拒绝排/撤/改
RoundPhase::PhaseIII => true,
RoundPhase::Settling | RoundPhase::Locked => false,
```
```rust
// compute：折叠手动覆盖（按 (effective_at, sequence) last-wins）
let manual = phase_sets.iter().max_by(|a,b| (a.effective_at,a.sequence).cmp(&(b.effective_at,b.sequence))).map(|e| e.phase);
let effective_phase = manual.or_else(|| policy::phase_at(&cfg.round.phases, rec.timestamp_ms));
let locked = matches!(effective_phase, Some(RoundPhase::Settling) | Some(RoundPhase::Locked));
```
- `policy::phase_rejection` 增加 `Settling` 分支（统一"已锁定/结算中"拒绝文案）。
- 移除 `State.locked` / `Pipeline::is_locked` / `set_locked`；`/api/workflow` 的 `locked` 改由**阶段派生**（兼容字段，JSON 形状不变）。
- `RoundPhaseSet` 事件**仅管理员可写**（见 §5 安全）。

## 3. computed 缓存（读取层，落盘）

```jsonc
// data/computed/<round_id>.json
{ "round_id":"…", "config_revision":7, "last_seq":4211, "effective_phase":"settling",
  "events":[…], "eligibilities":[…], "board":{…}, "version":900,
  "steps":[…],            // 复盘：按消息组的分步(前缀)结果
  "updated_at":"…", "integrity_sha256":"…" }
```
- **键**：`(round_id, config_revision, last_seq)`；加载时校验 `integrity_sha256`。
- **失效**：`setup_round` / `put_config` / `put_settlement_config` 改变目录/配置 → 失效。
- **activate 策略**：`continue`=加载缓存；`fresh`=清空缓存；`replay`=忽略缓存全量重算。
- `data/computed/` 落在 `/data/*`（gitignored），**不被 `ServeDir(web)` 暴露**；加载失败/哈希不符 → 重新计算，不信任磁盘缓存。

## 4. 增量 compute

- **追加** → 仅重算**尾部组**（分组顺序性）→ 拼接事件 → 全量 `rebuild_allocation_snapshot`（纯内存，无 LLM/IO）。
- **编辑 / 删除 / 改 `timestamp_ms` / override 刷新** → 从**最早受影响组**（无法判定则**全量**）。
- `version = events.len()` 在"全量重建"下稳定。
- **引擎级增量**作为后续纯性能优化（困难：分配是顺序依赖、非局部；需事件 apply/retract 与 ID/版本稳定），本版不做。

## 5. 安全加固（子代理审查强制项）

| # | 加固 | 落点 |
|---|---|---|
| F1 | 测试轮隔离设**硬约束**：`put_config`/`setup_round`(ok=false 拒写)/`activate_round` 强制 `category_issues`；`test_group_id` 必须独立且≠任何正常轮群号；删除 `sim_routes::test_group()` 的 `round.group_id` 回退 | `api/config_routes.rs`、`services/rounds.rs`、`api/sim_routes.rs` |
| F2 | 伪造信封**降权**：`is_admin=false`、**忽略内层 `timestamp`**（用外层/服务器时间）、身份严格按 `user_id` | `llm/pipeline/mod.rs::unwrap_forged_envelope` |
| F3 | `round_id` **硬校验**：配置入口 + 每个路径构造（messages/events/settlement/snapshot/**computed**） | `settings::rounds::is_valid_round_id` 前置 |
| F7 | **原子重算**：单一 store 级 RMW 锁覆盖 `read_all`→写回→computed 写；API 编辑/快照导入串行化 | `llm/pipeline` + `messages` |
| F8 | 入站**长度上限**（`MAX_TEXT`）+ WS/API body 上限 | `gateway/onebot`、`llm/pipeline` |
| F9/F10 | `ParseOverride`/`RoundPhaseSet` 来源校验（来源消息不可设阶段）；`/导出` 等副作用**仅 live**；`snapshot/import apply` 受锁；`computed` 完整性哈希 | 多处 |

## 6. 逐文件改造

| 文件 | 改动 |
|---|---|
| `domain/event.rs` | 新增 `RoundPhaseSet` |
| `round/mod.rs` | `can_claim/can_cancel`：Settling/Locked=false |
| `parser/policy.rs` | `phase_rejection` 加 Settling；`member_allowed`/`proxy_base` 保留 |
| `parser/coalesce` | 不变 |
| `replay/session/mod.rs` | **唯一处理器**：分组→解析(规则+override，不调 LLM)→校验/权限→事件；幂等判重；阶段折叠；增量入口；分步(按组)输出 |
| `llm/pipeline/mod.rs` | `ingest`：预处理全落盘→LLM override 落盘→增量 compute→写 computed；删 `merge_open`/`ingest_merged`/实时 `process` 双路径；F2 加固 |
| `llm/pipeline/admin.rs` | 管理员命令只落盘；`/锁位`→`RoundPhaseSet(Settling\|Locked)`、`/开团`→清除覆盖 |
| `llm/pipeline/llm_parse.rs` | `emit_parse_override` 泛化到所有 LLM 解析 |
| `llm/pipeline/replay_view.rs` | 读 computed 的**按组分步**结果 |
| `llm/pipeline/state.rs` | 删 `locked`/`is_locked`/`set_locked`；`apply_replay` 载入 computed |
| `services/computed.rs`（新） | computed 读写 + 失效 + 完整性 |
| `services/settlement.rs` | `put_draft`(:147) 去掉锁门禁（试算/分包不阻塞）；`lock_draft` 写 `RoundPhaseSet(Locked)` |
| `services/messages.rs` | 编辑/删除/reroute：受阶段锁门禁（423）；改时间戳→全量重算 |
| `services/rounds.rs` | `activate` 三模式缓存策略；配置变更失效 computed |
| `settings/mod.rs` + `config.example.json` | `merge.enabled` 默认 true |
| `api/*` | 读取按 `seq/version` 增量；`workflow.locked` 由阶段派生 |
| `main.rs` | 启动：加载 computed 或增量重算（替换 `restore_persistent_lock`） |

## 7. 测试

- 单元：`can_claim` Settling/Locked=false；`RoundPhaseSet` 折叠（last-wins）；computed 读写/哈希/失效；`round_id` 校验；`unwrap` 降权；长度上限。
- parity：锁（`/锁位` 后拒绝、解锁后**不复活**——由阶段事件复现）；幂等（重复 message_id→`Duplicate`）；具名代排；匿名代排（按发送者 QQ）；合并（相邻同意图）；编辑/删除后重算。
- e2e：开合并用例；锁/阶段断言改由 `workflow.locked`。
- 全量：`cargo test` / `clippy -D warnings` / `node tests/e2e/sim.mjs` / `npm run privacy`。

## 8. 历史Git提议（暂不执行，需专项确认）

以下是当时的调查提议，不是当前事实或执行授权。“全部提交未签名”已不符合目前G签名提交；任何历史改写必须另行核实泄露范围、备份与协作者影响，不因继续开发自动执行或force-push。

- 仓库历史已泄露真实群号、约 32 个真实昵称、内网 IP；且**全部提交未签名**（`commit.gpgsign=true` 但历史为 `N`）。
- 步骤：① `git filter-repo --replace-text <清洗表>`（用 `data/members.json` 真实昵称 + `config/app.json` 6+位值 + 内网 IP 生成替换表；仅改历史，不动 HEAD）→ ② 校验 `git log --all -S` 0 命中 → ③ **重签全部分支提交**（`git filter-repo --commit-callback` 或 `git rebase --root -S`）→ ④ 校验 `%G?` 全 `G` → ⑤ **协调 force-push**（共享分支）。

## 9. 已放弃或停止直接沿用的旧功能/思路

- “合并默认开”：由现行实验默认关取代。排/撤不同语义，既存组按理解产物冻结，不重新按当前开关拆并。
- “消息按seq/排位按事件数即可增量”：已由独立投影epoch/revision取代，旧行修订不得漏刷新。
- 日常修改/删除原消息触发解析、批量LLM覆盖：由保留原文/自动结果、完整人工动作与可选草稿代填取代。
- 简单删除持久锁、使锁后订单可写、仅last_seq缓存键：不再是可直接执行的代码清单。必须保留现行持久锁、来源/修订守卫和恢复门禁；computed只作可重建派生物。
- 强行把复盘折叠成组末一步：与原消息位置/逐条核对需求须重新对照，不借性能重构删除消息可达性。
- 测试信封时间/管理员权限变更、历史重写不是本目标自动授权范围；保持测试群隔离和已确认排序合同，争议项单列待确认。
