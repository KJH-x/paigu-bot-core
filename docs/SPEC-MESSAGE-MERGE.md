# SPEC · 临近同发送者消息合并理解 + 匿名代排（2026-10-13）

> 状态：**已实现（2026-10-13）**。口径见 [DECISIONS.md](./DECISIONS.md) §B-5；工程动作见 [TODOS.md](./TODOS.md)「消息合并（2026-10-13）」。
> 实现按 §12 校验修正为准；已同步 [INTERFACES.md](./INTERFACES.md) / [FUNCTIONAL.md](./FUNCTIONAL.md) / [POLICY.md](./POLICY.md) / [DESIGN.md](./DESIGN.md) / [MODULES.md](./MODULES.md) / [AGENTS.md](../AGENTS.md) / [README.md](../README.md)。

## 1. 目标

同一发送者在消息流中相邻、**同意图**的多条消息（无他人插话），视为同一理解单元；排、撤、改不能跨意图合并。处理不推迟（首条立即处理，后续同意图补充到达即更新）。匿名代排 `X(代)` 表示匿名、代群外的代排，行内填 `发送者(代)`。

2026-10-16 用户补充（DECISIONS H-8）：甲排 A 得到位次 B，只要没有再次加排 A，之后撤 A 就撤掉原认购；紧跟/不紧跟、夹杂闲聊/其他商品/他人发言均不改变这层关系。撤销是自己原时刻的动作，不拼进前条申购。人工补充性合并另见 [专项计划](./PLAN-message-reprocess-2026-10-15.md)，不是放开跨意图自动合并。

## 2. 最终口径（R1–R12）

| # | 口径 |
|---|---|
| R1 | **无时间窗口**：合并只按"消息流相邻"判定；**被其他 user 打断即断开**（不再跟踪）。 |
| R2 | **同意图**才合并：`Claim` / `Cancel` / `Modify` 三者各自成组、彼此不跨组；**普通闲聊/无明确意图（含"想要"这类模糊表述）→ `Ignored`（保持现状），单例且断开当前串**；无明确意图**不并入**相邻组。 |
| R3 | `代排` 视为 **Claim 的修饰**：含 `排`/`claim`/`代排`/`(代)` 的文本归为 Claim 意图（用于分组）。 |
| R4 | 组文本 = 成员按时间序 `\n` 连接；**代表 = 最后一条**（其 `message_id`/`timestamp_ms` 用于阶段/优先/归属）。 |
| R5 | **处理不推迟**：首条立即处理；后续补充到达即**更新**（同一分组重算）。 |
| R6 | **测试 == 实际**：sim 注入与 NC 走**同一入口**（接收/预处理/实时计算一致）。 |
| R7 | **实时 == 重放**：分组为对日志的**确定性函数**，实时与重放共用同一纯函数（`src/parser/coalesce.rs`）。 |
| R8 | **匿名代排 `X(代)`**：该行填 **`发送者(代)`**（合成身份，匿名、代群外）；**校验按代排者（发送者）**——发送者在阶段/优先/群内可排 ⇒ 该项可排；**锁定仍生效**。 |
| R9 | 边界（各自单例，且断开当前串）：管理员、`/` 开头命令、空文本、`routed != "message"`（drop）。 |
| R10 | **消息流展示不合并**：`/api/display`、`/api/replay/view` **逐条**输出；成员行标 `Merged`（显示"已合并"徽标），代表行显示本条**原始**文本。 |
| R11 | 配置：`merge.enabled`（默认关，实验特性）+ `merge.same_sender`；顶层 `AppConfig.merge`。 |
| R12 | 仓库根 `README.md` 增加开发中告警。 |

## 3. 数据模型 / 配置

```rust
// src/settings/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MergeSettings {
    /// 总开关（实验特性，默认关）。
    #[serde(default)]
    pub enabled: bool,
    /// 同发送者合并（唯一模式；默认 true，`enabled` 为总闸）。
    #[serde(default = "default_true")]
    pub same_sender: bool,
}
// AppConfig { …, #[serde(default)] pub merge: MergeSettings }
```
`config.example.json`：
```jsonc
"merge": { "enabled": false, "same_sender": true }
```

状态词表新增 **`Merged`**（成员行）。计数/展示需容忍：`messages_since` 不过滤（逐条展示）、`recompute` 计数加入 `merged`、前端加标签。

## 4. 共享纯分组函数（`src/parser/coalesce.rs`，新增）

```rust
use crate::domain::item::Item;
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::rule_parser::RuleParser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupIntent { Claim, Cancel, Modify }

/// 一条候选：调用方须按 `(timestamp_ms, seq)` 排序、且已过滤 `routed=="message"`。
pub struct Candidate<'a> {
    pub user_id: &'a str,
    pub text: &'a str,
    pub is_admin: bool,
}

fn is_boundary(c: &Candidate<'_>) -> bool {
    let t = c.text.trim();
    c.is_admin || t.starts_with('/') || t.is_empty()
}

fn classify(c: &Candidate<'_>, items: &[Item]) -> Option<GroupIntent> {
    if is_boundary(c) { return None; }
    let t = c.text.trim();
    let proxy_marker = t.contains("代排") || t.contains("(代)") || t.contains("（代）");
    match RuleParser::parse(t, items, c.is_admin).intent {
        _ if proxy_marker => Some(GroupIntent::Claim),
        ParsedIntent::Claim => Some(GroupIntent::Claim),
        ParsedIntent::Modify => Some(GroupIntent::Modify),
        ParsedIntent::Cancel => Some(GroupIntent::Cancel),
        _ => None, // Other/Unknown/AdminCommand → 单例、断开
    }
}

/// 返回分组；每组是候选下标的有序列表（长度 1 = 独立）。
pub fn group_candidates(cands: &[Candidate<'_>], items: &[Item]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut key: Option<(String, GroupIntent)> = None;

    macro_rules! flush { () => { if !cur.is_empty() { groups.push(std::mem::take(&mut cur)); } }; }

    for (i, c) in cands.iter().enumerate() {
        match classify(c, items) {
            Some(kind) => {
                if key.as_ref().map_or(false, |(u, k)| u == c.user_id && *k == kind) {
                    cur.push(i);
                } else {
                    flush!();
                    key = Some((c.user_id.to_string(), kind));
                    cur.push(i);
                }
            }
            None => { flush!(); key = None; groups.push(vec![i]); }
        }
    }
    flush!();
    groups
}

/// 把一组候选的文本按时间序拼成理解单元。
pub fn joined_text(cands: &[Candidate<'_>], group: &[usize]) -> String {
    group.iter().map(|&i| cands[i].text.trim()).collect::<Vec<_>>().join("\n")
}
```

## 5. 重放路径（`src/replay/session/mod.rs::compute`）

在 `ordered`（现按 `(timestamp_ms, seq)` 排序）之上分组；仅对**组代表**跑 `process_one`，成员输出 `Merged`。

```rust
// 仅对 routed=="message" 的 ordered 位置做分组；非 message 记录各自单例。
let msg_idx: Vec<usize> = ordered.iter().enumerate()
    .filter(|(_, r)| r.routed.eq_ignore_ascii_case("message"))
    .map(|(i, _)| i).collect();
let cands: Vec<Candidate> = msg_idx.iter()
    .map(|&i| Candidate { user_id: &ordered[i].user_id, text: &ordered[i].text, is_admin: ordered[i].is_admin })
    .collect();
let groups = group_candidates(&cands, &items);
// groups 的下标是 cands 下标 → 映射回 ordered 下标 msg_idx[k]
```
对每个组：
- 单成员 → 照旧 `process_one(&ordered[pos])`。
- 多成员 → 构造合成 `MessageRecord{ text: joined_text(...), ..ordered[rep] }`，`process_one(&synthetic)` 一次；再为**每个**成员产出 `ReplayMessageView/Outcome`：代表=真实 `processed`，其余 `status="Merged"`、`detail="并入 #<rep.seq>"`。
- 非 message 记录 → 照旧逐条（drop 等）。

> `messages/outcomes` 保持与 `records` 1:1（`recompute` 按 `record_id` 回写）。`process_one` 原本对 `routed` drop 的短路照旧处理单例。

## 6. 实时路径（`Pipeline`）

### 6.1 统一入口
```rust
impl EventSink for Pipeline {
    async fn handle(&self, ev: IncomingEvent) { self.ingest(ev).await; }
}

impl Pipeline {
    /// 接收入口（NC / 测试注入共用）。
    pub async fn ingest(&self, ev: IncomingEvent) {
        let cfg = self.cfg.get().await;
        let ev = gate_nc(&cfg, ev);           // 仅 NC 伪造；sim 传入即结构化消息
        if !cfg.merge.enabled {
            let _ = self.process(ev).await;   // 旧快路径（逐条）
            return;
        }
        let _ = self.ingest_merged(cfg, ev).await;
    }
}
```

### 6.2 合并摄取（急切 + 更新）
```rust
// Pipeline 新增字段：merge_open: tokio::sync::Mutex<Option<OpenGroup>>
struct OpenGroup { user_id: String, intent: GroupIntent, message_ids: Vec<String>, texts: Vec<String> }

async fn ingest_merged(&self, cfg: AppConfig, mut ev: IncomingEvent) -> PipelineOutcome {
    let items = cfg.round.to_items();
    let kind = classify_public(&ev, &items); // 复用 coalesce 的 classify（pub(crate)）
    // 匿名代排：改写身份为 `发送者(代)`，但权限仍按发送者。
    let (owner, sender_priority) = self.apply_anonymous_proxy(&cfg, &mut ev);

    match kind {
        Some(k) => {
            // 追加或新建当前组
            let joined = {
                let mut open = self.merge_open.lock().await;
                match open.as_mut() {
                    Some(g) if g.user_id == ev.user_id && g.intent == k => {
                        g.message_ids.push(ev.message_id.clone()); g.texts.push(ev.text.clone());
                    }
                    _ => {
                        *open = Some(OpenGroup { user_id: ev.user_id.clone(), intent: k,
                            message_ids: vec![ev.message_id.clone()], texts: vec![ev.text.clone()] });
                    }
                }
                let g = open.as_ref().unwrap();
                (g.texts.join("\n"), g.message_ids.last().cloned().unwrap())
            };
            // 1) 落原始记录（状态占位，随后 recompute 回写）
            self.persist_status(&ev, "Pending", "pending", owner.as_deref()).await;
            // 2) LLM 解析（若需）→ 落 ParseOverride（键=代表 message_id），保证重放一致
            if cfg.llm.enabled && looks_like_order(&joined, &RuleParser::parse(&joined, &items, ev.is_admin)) {
                if let Ok(parsed) = self.llm_parse(&cfg, &IncomingEvent{ text: joined.clone(), ..ev.clone() }).await {
                    self.emit_parse_override(&cfg, &ev, /*seq*/ 0, Utc::now(), &parsed).await;
                }
            }
            // 3) 权威重算（新消息与追加同源）
            self.recompute().await
        }
        None => {
            self.merge_open.lock().await.take();     // 断开当前串
            let _ = self.process(ev).await;          // 闲聊/命令/边界：旧路径
            self.recompute_silent().await
        }
    }
}
```

关键点：
- **追加更新 == 新消息更新**：两者都走 `persist + (可选 LLM ParseOverride) + recompute`，同源 ⇒ 一致。
- **`state.seq`/`seen`**：由 `recompute` 的 `apply_replay` 从日志重建（`seq = max(records.seq)`、`seen` 重建全部键），一致。
- **不确定尾部**：`recompute` 以整条日志为准，尾部组由"日志里是否还有同人同意图相邻"确定，天然正确、无需墙钟。

### 6.3 `Pipeline::recompute`
把 `src/services/replay.rs::recompute` 的核心抽成 `Pipeline::recompute(&self) -> anyhow::Result<Value>`（读日志 → `session::replay` → 回写状态 → `apply_replay`），`services::replay::recompute` 复用它，避免两套。

## 7. 匿名代排（`X(代)`）

- **识别**：文本含 `代排` / `(代)`；`RuleParser` 判为 Claim（R3）。
- **身份改写**：`ev.user_id = format!("{sender_display}(代)")`, `ev.nickname = ""`（沿用既有 `发送者(代名称)` 机制，名称留空）。
- **权限按发送者**：`is_priority` 用**原始发送者** `id/nickname` 计算；阶段/优先窗口校验使用该 `is_priority`（`policy::in_prioritiy_at` + `phase_rejection(..., is_priority)`），**锁定仍生效**。
- **展示**：slot/who-whats 显示 `发送者(代)`；消息流仍逐条（R10）。

```rust
// src/llm/pipeline/mod.rs（process 内，身份解析后）
let (perm_uid, perm_nick) = (ev.user_id.clone(), ev.nickname.clone());
if is_anonymous_proxy(&ev.text) {
    let display = crate::settings::resolve_cn(&cfg, &ev.user_id, &ev.nickname)
        .unwrap_or_else(|| ev.user_id.clone());
    ev.user_id = format!("{display}(代)");
    ev.nickname = String::new();
}
// …此前用 `policy::is_priority(&cfg, &perm_uid, &perm_nick)` 取代 `&ev.user_id`
```

## 8. 展示

- `/api/display`（`state::messages_since`）与 `/api/replay/view`（`replay_view.rs`）：**逐条**输出；`Merged` 不过滤、按普通行显示（徽标"已合并"）。
- 前端：`web/display.js` 状态映射加 `merged`；`web/display.css`/`theme.css` 加 `.st.merged`；`web/replay.js` 映射加 `Merged`。
- 代表行文本为**原始**最后一条（不是拼接串），避免"合并展示"。

## 9. README 告警（仓库根 `README.md`）

> **Warning:** This project is under active development — large, unexpected diffs may appear at any time as message merging (same-sender coalescing) evolves. Contribute at your own risk.

## 10. 测试

1. **纯函数** `src/parser/coalesce.rs` 单测：同人同意图相邻合并；换人断；`/`/空/管理员边界；闲聊(Unknown)单例且断；`想要`→非 claim；`代排`/`(代)`→Claim。
2. **重放分组** `src/replay/session/tests.rs`：多成员组 → 代表真实、成员 `Merged`；组文本=join。
3. **实时==重放 parity**（仿 `realtime_and_replay_modify_boards_match`）：`merge.enabled=true`、`DisabledLlm`；喂相邻多条同人同意图；断言 `version` 与 `canonical_board` 相等、逐条状态一致（含 `Merged`）。
4. **追加更新==新消息更新**：先逐条（新）后追加（合并）两条路径结果一致。
5. **匿名代排**：`X(代)` → 行填 `发送者(代)`；阶段/优先按发送者有权限则 Applied、无权限则 Rejected；锁定仍拒绝。
6. **e2e** `tests/e2e/sim.mjs`：适配（默认 `merge.enabled=false` 不受影响；新增一条 merge 用例）。

## 11. 风险与不变式

- **性能**：`merge.enabled` 下每条消息触发 `recompute`（O(消息数)）；实验特性，README 已告警。后续可做"仅重算尾部组"的增量优化。
- **LLM parity**：合并组的 LLM 解析结果以 `ParseOverride`（键=代表 `message_id`）落库，重放消费 ⇒ 一致（同时补齐既有 LLM parity 洞）。
- **重复消息**：`recompute` 的重放无 `seen` 去重（既有）；合并路径以日志为准，重复记录仍会各自分组（同人同意图 → 可能并入）——按现口径接受。
- **锁定 / drop / 白名单**：沿用 `process_one` 既有短路。

## 12. 校验修正（子代理，2026-10-13）

子代理校验后对 §4–§7 的**强制修正**（实现按此为准）：

1. **分组分类优先级**：先取 `RuleParser` 意图（Cancel/Modify/Claim）；仅当为 `Unknown` 且文本含 `排`/`claim`/`代排`/`(代)` 时才升级为 Claim。避免 `取消 代排` 被误判为 Claim。（R3 改为："含 `排`/`claim`/`代排`/`(代)` 的**未知意图**文本升级为 Claim"。）
2. **R9 改写**：drop 记录在分组前**已被过滤**（网关的 drop 从不进入 Pipeline），故 drop **不作为边界**；边界仅为 `管理员`、`/` 命令、`空文本`。
3. **代表选择两路径一致**：代表 = 组内 `(timestamp_ms, seq)` **最大**者（实时 `merge_open` 也按此选，不用到达序），`ParseOverride` 键=该代表 `message_id`。
4. **代表展示文本**：视图/落盘用代表**原始文本**；仅**解析**用 `joined_text`（合成记录仅用于 `process_one` 输入，不用于视图 `text`）。
5. **`Pipeline::recompute` 返回 `ReplayResult`**（供 `remember_replay`/summary/diff 复用）；**`llm::pipeline` 不得依赖 `services`**（只用 `replay::session`）。
6. **`ingest` 返回 `PipelineOutcome`**；保留 `append_raw_event`（原始事件）；`emit_parse_override` 提升为 `pub(super)`。
7. **`merge_open` 失效**：存 `round_id`+`revision`，二者变化即清；`apply_replay`/`reset` 时清（覆盖外部编辑触发的 `recompute`）。
8. **匿名代排需要持久化发送者身份**：`MessageRecord` 新增 `#[serde(default)] sender_user_id: Option<String>` 与 `sender_nickname: Option<String>`；`process`/`process_one`/`ingest_merged` 共用**同一个** `(代)` 身份助手；`is_priority` 与 `member_allowed` 用 `sender_*`（无则回退 `user_id`/`nickname`），展示/归属用合成 `user_id`。助手须**幂等**（已是 `(代)` 不再改写）。
9. **锁定**：`ingest_merged` 若 `state.locked` 则走 `process`（拒绝）且**不 recompute**（保证"锁定仍生效"）。
10. **计数补齐**：`Merged` 加入 `services/replay.rs` summary 与 `api/sim_routes.rs::Summary`；前端 `display.js`/`replay.js` 映射加 `Merged`；CSS `.st.merged`。
11. **sim 走 `ingest`**（`sim_routes.rs:183/220`）——R6 前提。
12. 文档索引：`AGENTS.md` §6 增补本 SPEC 链接。

## 13. 验收与遗留（2026-10-13）

**已实现并通过**：`cargo test` 230 · `cargo clippy -- -D warnings` · e2e 17/17 · `npm run privacy` OK。

- **§12.8 落地方式**：新增 `bus::IncomingEvent.sender_user_id/sender_nickname` 与 `messages::MessageRecord.sender_user_id/sender_nickname`（`#[serde(default)]`）；`ingest` 匿名代排改写时写入发送者原身份，`process`/`process_one` 的 `is_priority`/`member_allowed` 改按 `sender_*`（无则回退 `user_id`/`nickname`）。→ **R8 按发送者（含按 QQ 号）判定**。
- **C2（部分）**：`OpenGroup` 增 `revision`，配置/轮次/身份/意图变化即重建组；`ingest` 串行化（`ingest_lock`）避免并发 `recompute` 读改写竞态。

**已知遗留（实验特性，`merge.enabled` 默认关；记录待办）**：
1. **外部编辑 vs `merge_open`**：`PUT/DELETE /api/messages` 触发的 `recompute` 不清 `merge_open`；极端情况下后续追加会用到旧成员文本。建议在编辑入口调 `Pipeline::invalidate_merge_open()`。
2. **锁定**：锁仅存在于内存，未入日志；解锁后的下一次 `recompute` 会把锁定期被拒的记录重算为 `Applied`（**既有问题**，合并路径因逐条 `recompute` 放大）。需将锁持久化或让 `compute` 感知锁。
3. **具名代排回放**：`代X排:` 仅实时 `process` 切段，`process_one`（含重放）不切 → 实时归属被代理人、重放归属发送者（**T-29 既有 parity 缺口**，非本特性引入）；合并开启后该消息被当普通 Claim。需在重放侧复用 `proxy_segments`。
4. **边界/闲聊 + LLM**：`ingest_merged` 的 Boundary/Other 分支走 `process`（可能 LLM）且不 `recompute`，后续某条 Join 的 `recompute` 用规则重算该记录 → 可能由 `Applied` 变 `Ignored`。需让所有消息同源（统一持久化 + LLM ParseOverride 落库）。
5. **重复 `message_id`**：合并路径未查 `seen`（旧快速路径会去重）；建议合并前查 `seen`。
6. **e2e**：未新增"合并"用例（Rust parity 单测已覆盖；sim 已走 `ingest`）。
