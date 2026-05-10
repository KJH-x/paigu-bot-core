# 排谷系统补充设计文档：事件重放、图形化审计与消息队列模拟排谷

本文档是原 Rust 排谷系统架构文档的补充，不修改原有架构，只扩展“事件重放系统”“前端图形化回放”“消息队列文件模拟排谷”三部分。核心目标是让系统具备可复现、可解释、可审计、可调试能力。

核心设计原则：任何一次排谷结果都必须能由同一批输入消息、同一份团配置、同一套规则版本，在任意时间重新计算得到完全一致的结果。前端看到的每一格变化，都应该能追溯到具体原始消息、解析事件、业务规则和计算步骤。


一、为什么要把事件重放做成一等功能

排谷场景天然容易产生争议。争议通常不是“最终表格长什么样”，而是“为什么这个人排在这里”“为什么这个撤销导致后面的人前移”“为什么包尾没有挡住散户”“为什么赠品被分给了这个人”“为什么某条消息没有识别出来”。

如果系统只保存最终表格，就无法解释这些问题。事件重放系统的价值是：

1. 可以重放任意时间点的排队状态。
2. 可以看到每条消息如何改变系统状态。
3. 可以定位某一次错误解析或错误规则。
4. 可以在不连接 QQ 的情况下，用消息队列文件模拟整场拼团。
5. 可以把争议从“口说无凭”变成“可视化证据”。
6. 可以为测试提供稳定输入输出。

所以补充设计里需要新增三个能力：Replay Engine、Timeline Snapshot、Simulation Runner。


二、整体补充架构

原有架构：

QQ 消息 -> RawMessage -> ParsedEvent -> ValidatedEvent -> EventStore -> AllocationSnapshot -> SettlementSnapshot -> FrontendSnapshot / Export

补充后架构：

Message Queue File / QQ Live Message
-> RawMessage Normalizer
-> Parser
-> Validation Layer
-> EventStore
-> Replay Engine
-> Timeline Snapshot Store
-> Frontend Replay Viewer
-> Diff Inspector
-> Simulation Report

其中 Replay Engine 不应该是一套新业务逻辑，而应该复用原来的 Allocation Engine 和 Settlement Engine。也就是说，系统只有一个排队计算器，只是它既可以用于实时运行，也可以用于历史重放和离线模拟。

重要约束：实时模式和模拟模式必须使用同一套代码路径。不能写一个“线上排队逻辑”和一个“模拟排队逻辑”。否则两者迟早不一致。


三、新增 Rust 模块建议

建议在原项目里增加以下模块：

src/
  replay/
    mod.rs
    replay_engine.rs
    replay_cursor.rs
    replay_options.rs
    timeline_snapshot.rs
    state_diff.rs
    replay_report.rs
  simulation/
    mod.rs
    queue_file.rs
    simulation_runner.rs
    simulation_report.rs
    fixtures.rs
  audit/
    mod.rs
    decision_trace.rs
    rule_trace.rs
    parse_trace.rs
    allocation_trace.rs
  api/
    replay_routes.rs
    simulation_routes.rs
  storage/
    timeline_store.rs

模块职责：

replay_engine.rs：按事件顺序重放事件，产出每一步状态。
replay_cursor.rs：表示当前回放进度，例如第几条事件、哪个时间戳、哪个版本。
replay_options.rs：控制是否包含解析失败事件、是否自动跳过无效事件、是否生成结算快照。
timeline_snapshot.rs：存储每一步的系统状态快照。
state_diff.rs：计算相邻快照之间的差异。
replay_report.rs：生成回放总结。
queue_file.rs：读取消息队列文件，转成 RawMessage。
simulation_runner.rs：运行离线模拟。
decision_trace.rs：记录系统为什么这样处理。
rule_trace.rs：记录命中的业务规则。
parse_trace.rs：记录 LLM 解析输入输出。
allocation_trace.rs：记录排队引擎每一步分配原因。
timeline_store.rs：持久化 timeline snapshots。


四、事件重放的核心数据结构

事件重放不应该只保存最终结果，还要保存每一步的“输入、输出、差异、解释”。建议定义 ReplayStep。

Rust 示例：

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStep {
    pub replay_id: String,
    pub round_id: String,
    pub step_index: u64,
    pub event_id: String,
    pub raw_message_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub input_event: ValidatedEvent,
    pub before_version: u64,
    pub after_version: u64,
    pub state_diff: StateDiff,
    pub allocation_snapshot: AllocationSnapshot,
    pub settlement_snapshot: Option<SettlementSnapshot>,
    pub decision_trace: DecisionTrace,
    pub warnings: Vec<ReplayWarning>,
    pub errors: Vec<ReplayError>,
}
```

StateDiff 用于前端高亮“这一条消息改变了什么”。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    pub slot_changes: Vec<SlotChange>,
    pub claim_changes: Vec<ClaimChange>,
    pub user_total_changes: Vec<UserTotalChange>,
    pub item_total_changes: Vec<ItemTotalChange>,
    pub settlement_changes: Vec<SettlementChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotChange {
    pub item_id: String,
    pub box_index: u32,
    pub slot_index: u32,
    pub before: Option<SlotView>,
    pub after: Option<SlotView>,
    pub reason: SlotChangeReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlotChangeReason {
    NewClaimFilled,
    CancelReleased,
    AutoMovedForward,
    TailSegmentCreated,
    TailSegmentUpdated,
    AdminFixed,
    AdminUnlocked,
    RecomputedByRuleChange,
}
```

DecisionTrace 是系统解释能力的关键。它不只是日志，而是结构化的解释。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTrace {
    pub parse_trace: Option<ParseTrace>,
    pub validation_trace: Vec<ValidationTraceItem>,
    pub priority_trace: Option<PriorityTrace>,
    pub allocation_trace: Vec<AllocationTraceItem>,
    pub settlement_trace: Vec<SettlementTraceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityTrace {
    pub user_id: String,
    pub priority_level: i32,
    pub matched_eligibility_ids: Vec<String>,
    pub sort_key: ClaimSortKey,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationTraceItem {
    pub claim_id: String,
    pub item_id: String,
    pub quantity_requested: u32,
    pub quantity_allocated: u32,
    pub policy: SlotPolicy,
    pub candidate_slots_checked: Vec<CandidateSlotTrace>,
    pub final_slots: Vec<SlotRef>,
    pub explanation: String,
}
```

注意 explanation 是给人看的，但系统判断不能依赖 explanation。真正判断必须依赖结构化字段。


五、Replay Engine 的执行方式

Replay Engine 的输入不是原始 QQ 消息，而是 ValidatedEvent。如果要从消息队列文件开始模拟，则先通过 Parser 和 Validation Layer 把 RawMessage 转成 ValidatedEvent，再交给 Replay Engine。

Replay Engine 的伪代码：

```rust
pub struct ReplayEngine {
    pub allocation_engine: AllocationEngine,
    pub settlement_engine: SettlementEngine,
}

impl ReplayEngine {
    pub async fn replay(
        &self,
        round_config: RoundConfig,
        mut events: Vec<ValidatedEvent>,
        options: ReplayOptions,
    ) -> Result<ReplayResult, ReplayError> {
        events.sort_by(|a, b| compare_event_order(a, b));

        let mut state = RuntimeState::new(round_config);
        let mut steps = Vec::new();
        let mut previous_snapshot = state.to_allocation_snapshot();

        for (index, event) in events.into_iter().enumerate() {
            let before_version = state.version;
            let before_snapshot = state.to_allocation_snapshot();

            let decision_trace = self.apply_event_with_trace(&mut state, &event).await?;

            let after_snapshot = state.to_allocation_snapshot();
            let state_diff = StateDiff::from_snapshots(&before_snapshot, &after_snapshot);

            let settlement_snapshot = if options.include_settlement {
                Some(self.settlement_engine.compute(&state).await?)
            } else {
                None
            };

            let step = ReplayStep {
                replay_id: options.replay_id.clone(),
                round_id: state.round_id.clone(),
                step_index: index as u64,
                event_id: event.event_id().to_string(),
                raw_message_id: event.raw_message_id().map(|x| x.to_string()),
                occurred_at: event.occurred_at(),
                input_event: event,
                before_version,
                after_version: state.version,
                state_diff,
                allocation_snapshot: after_snapshot.clone(),
                settlement_snapshot,
                decision_trace,
                warnings: vec![],
                errors: vec![],
            };

            steps.push(step);
            previous_snapshot = after_snapshot;
        }

        Ok(ReplayResult {
            replay_id: options.replay_id,
            final_snapshot: previous_snapshot,
            steps,
        })
    }
}
```

关键点是 apply_event_with_trace。它需要一边执行业务逻辑，一边记录为什么。

```rust
impl ReplayEngine {
    async fn apply_event_with_trace(
        &self,
        state: &mut RuntimeState,
        event: &ValidatedEvent,
    ) -> Result<DecisionTrace, ReplayError> {
        match event {
            ValidatedEvent::Claim(claim) => {
                let priority_trace = state.resolve_priority_trace(&claim.user_id, &claim.round_id);
                let allocation_trace = self.allocation_engine.apply_claim_with_trace(state, claim).await?;
                Ok(DecisionTrace {
                    parse_trace: claim.parse_trace.clone(),
                    validation_trace: claim.validation_trace.clone(),
                    priority_trace: Some(priority_trace),
                    allocation_trace,
                    settlement_trace: vec![],
                })
            }
            ValidatedEvent::Cancel(cancel) => {
                let allocation_trace = self.allocation_engine.apply_cancel_with_trace(state, cancel).await?;
                Ok(DecisionTrace {
                    parse_trace: cancel.parse_trace.clone(),
                    validation_trace: cancel.validation_trace.clone(),
                    priority_trace: None,
                    allocation_trace,
                    settlement_trace: vec![],
                })
            }
            ValidatedEvent::AdminAdjust(adjust) => {
                let allocation_trace = self.allocation_engine.apply_admin_adjust_with_trace(state, adjust).await?;
                Ok(DecisionTrace {
                    parse_trace: adjust.parse_trace.clone(),
                    validation_trace: adjust.validation_trace.clone(),
                    priority_trace: None,
                    allocation_trace,
                    settlement_trace: vec![],
                })
            }
            ValidatedEvent::SettlementRule(rule) => {
                state.settlement_rules.push(rule.clone());
                state.version += 1;
                Ok(DecisionTrace::empty_with_note("更新结算规则"))
            }
        }
    }
}
```


六、事件排序规则必须可解释

手速团最敏感的是排序。Replay Viewer 必须显示排序依据。建议每个 ValidatedEvent 都有 EventOrderKey。

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct EventOrderKey {
    pub priority_level: i32,
    pub effective_timestamp_ms: i64,
    pub source_sequence: u64,
    pub message_id: String,
}
```

排序规则：

```rust
pub fn compare_event_order(a: &ValidatedEvent, b: &ValidatedEvent) -> std::cmp::Ordering {
    let ak = a.order_key();
    let bk = b.order_key();

    bk.priority_level
        .cmp(&ak.priority_level)
        .then_with(|| ak.effective_timestamp_ms.cmp(&bk.effective_timestamp_ms))
        .then_with(|| ak.source_sequence.cmp(&bk.source_sequence))
        .then_with(|| ak.message_id.cmp(&bk.message_id))
}
```

这里 priority_level 是倒序，时间戳是正序。前端要把这个排序键展示出来，尤其是两个用户争同一个格子时。建议 UI 显示：

用户 A：优先级 10，时间 20:00:01.123，队列序号 15
用户 B：优先级 0，时间 20:00:00.900，队列序号 14
结果：A 先排，因为优先级更高

如果管理员配置为“优先权只在开团前生效”，则 DecisionTrace 里要显示该优先权是否生效。


七、前端图形化回放设计

前端 Replay Viewer 建议分成五个区域。

顶部：回放控制栏。

包含 round 选择、replay_id、当前 step、时间戳、播放/暂停、上一步、下一步、跳到指定消息、速度选择、只看某用户、只看某商品、只看错误事件。

左侧：事件时间轴。

每条事件显示：时间、用户、意图、简短摘要、状态。状态包括成功、部分成功、解析待确认、无效、管理员覆盖、撤销、结算规则更新。点击事件后，中间表格跳到该事件后的状态。

中间：排谷表格视图。

按商品展示盒和槽位。当前 step 产生变化的格子高亮。新增填入、撤销释放、自动前移、锁位创建、管理员固定应该使用不同图标或边框。不要只用颜色，因为截图或色弱场景会不清楚。

右侧：决策解释面板。

显示原始消息、LLM 解析结果、校验结果、优先权匹配、排序键、排位过程、结算影响。用户争议时，这个面板就是证据。

底部：Diff 面板。

显示本 step 前后变化。例如：

事件：用户 456 排 燐音吧唧 x2
变化：
燐音吧唧 box1 slot3：空 -> 用户456
燐音吧唧 box1 slot4：空 -> 用户456
用户456 应付：0 -> 90
商品燐音吧唧 已排数量：2 -> 4


八、Timeline Snapshot 的存储策略

如果每一步都保存完整快照，数据会很大，但实现简单。如果每一步只保存 diff，回放任意时间点需要从头还原，速度较慢。建议两者结合。

策略：

1. 每一步保存 StateDiff。
2. 每隔 N 步保存完整 AllocationSnapshot，例如每 50 步。
3. 用户跳到任意 step 时，从最近的完整快照开始应用 diff。
4. 如果团规模不大，也可以全量保存每一步快照，先保证实现简单。

Rust 数据结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSnapshotRecord {
    pub replay_id: String,
    pub round_id: String,
    pub step_index: u64,
    pub snapshot_kind: SnapshotKind,
    pub full_snapshot: Option<AllocationSnapshot>,
    pub state_diff: Option<StateDiff>,
    pub settlement_snapshot: Option<SettlementSnapshot>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotKind {
    Full,
    DiffOnly,
}
```

R2 路径建议：

rounds/{round_id}/replays/{replay_id}/manifest.json
rounds/{round_id}/replays/{replay_id}/steps/000000.json
rounds/{round_id}/replays/{replay_id}/steps/000001.json
rounds/{round_id}/replays/{replay_id}/snapshots/000000_full.json
rounds/{round_id}/replays/{replay_id}/snapshots/000050_full.json
rounds/{round_id}/replays/{replay_id}/final.json

manifest.json 示例：

```json
{
  "replay_id": "replay_20260508_001",
  "round_id": "es_2026_05",
  "created_at": "2026-05-08T21:00:00+08:00",
  "event_count": 312,
  "snapshot_interval": 50,
  "rule_version": "ruleset_2026_05_08_a",
  "parser_version": "parser_prompt_2026_05_08_a",
  "input_source": "queue_file",
  "final_snapshot_path": "final.json"
}
```


九、消息队列文件模拟排谷

你提出的软件需要“通过读取一段消息队列文件来执行模拟排谷”。这非常重要，建议定义一个稳定的文件格式，不要直接依赖 QQ 框架的原始格式。QQ 框架格式变化时，只改 adapter，不改模拟器。

推荐支持两种输入格式：JSON Lines 和 YAML。生产环境更适合 JSON Lines，人工编写测试更适合 YAML。

JSON Lines 格式，每一行是一条 RawMessage：

```json
{"source_sequence":1,"group_id":"10001","user_id":"u1","nickname":"甲","message_id":"m1","timestamp_ms":1778241601000,"text":"排燐音吧唧2","attachments":[]}
{"source_sequence":2,"group_id":"10001","user_id":"u2","nickname":"乙","message_id":"m2","timestamp_ms":1778241601100,"text":"排燐音吧唧1 蓝良1","attachments":[]}
{"source_sequence":3,"group_id":"10001","user_id":"u1","nickname":"甲","message_id":"m3","timestamp_ms":1778241610000,"text":"撤燐音1","attachments":[]}
```

Rust 结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMessageRecord {
    pub source_sequence: u64,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub timestamp_ms: i64,
    pub text: String,
    pub attachments: Vec<MessageAttachment>,
    pub reply_to_message_id: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttachment {
    pub attachment_type: String,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub sha256: Option<String>,
}
```

queue_file.rs 示例：

```rust
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn read_jsonl_queue_file(path: &str) -> Result<Vec<QueueMessageRecord>> {
    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("无法打开消息队列文件: {}", path))?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut records = Vec::new();
    let mut line_no = 0u64;

    while let Some(line) = lines.next_line().await? {
        line_no += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let record: QueueMessageRecord = serde_json::from_str(trimmed)
            .with_context(|| format!("第 {} 行不是合法 JSON", line_no))?;

        records.push(record);
    }

    records.sort_by(|a, b| {
        a.timestamp_ms
            .cmp(&b.timestamp_ms)
            .then_with(|| a.source_sequence.cmp(&b.source_sequence))
            .then_with(|| a.message_id.cmp(&b.message_id))
    });

    Ok(records)
}
```

RawMessage Normalizer：

```rust
impl From<QueueMessageRecord> for RawMessage {
    fn from(record: QueueMessageRecord) -> Self {
        RawMessage {
            raw_message_id: record.message_id.clone(),
            group_id: record.group_id,
            user_id: record.user_id,
            nickname: record.nickname,
            message_id: record.message_id,
            source_sequence: record.source_sequence,
            timestamp_ms: record.timestamp_ms,
            text: record.text,
            attachments: record.attachments.into_iter().map(Into::into).collect(),
            reply_to_message_id: record.reply_to_message_id,
            is_admin: record.is_admin,
            received_at_ms: record.timestamp_ms,
        }
    }
}
```


十、Simulation Runner 设计

Simulation Runner 是离线入口。它读取团配置、优先权表、商品表、消息队列文件，然后跑完整流程。

命令行建议：

cargo run -- simulate \
  --round-config ./fixtures/round_es_2026_05.yaml \
  --queue ./fixtures/messages.jsonl \
  --output ./out/replay_es_2026_05 \
  --include-settlement \
  --snapshot-interval 50

SimulationRunner 伪代码：

```rust
pub struct SimulationRunner {
    pub parser: Arc<dyn MessageParser>,
    pub validator: EventValidator,
    pub replay_engine: ReplayEngine,
    pub timeline_store: Arc<dyn TimelineStore>,
}

impl SimulationRunner {
    pub async fn run(&self, input: SimulationInput) -> anyhow::Result<SimulationReport> {
        let round_config = RoundConfig::load_from_file(&input.round_config_path).await?;
        let queue_records = read_jsonl_queue_file(&input.queue_path).await?;

        let mut validated_events = Vec::new();
        let mut parse_failures = Vec::new();
        let mut validation_failures = Vec::new();

        for record in queue_records {
            let raw_message: RawMessage = record.into();

            let parsed = match self.parser.parse(&round_config, &raw_message).await {
                Ok(x) => x,
                Err(err) => {
                    parse_failures.push(ParseFailure::from_error(&raw_message, err));
                    continue;
                }
            };

            match self.validator.validate(&round_config, raw_message, parsed).await {
                Ok(events) => validated_events.extend(events),
                Err(err) => validation_failures.push(err),
            }
        }

        let replay_result = self.replay_engine
            .replay(round_config, validated_events, input.replay_options)
            .await?;

        self.timeline_store.save_replay_result(&replay_result).await?;

        Ok(SimulationReport {
            replay_id: replay_result.replay_id,
            total_input_messages: replay_result.input_message_count,
            total_events: replay_result.steps.len() as u64,
            parse_failures,
            validation_failures,
            final_snapshot_path: self.timeline_store.final_snapshot_path(),
            manifest_path: self.timeline_store.manifest_path(),
        })
    }
}
```

注意一个细节：一条消息可能产生多个事件。例如“排燐音2，蓝良1，立牌单领1”会产生多个 ClaimLine，但可以属于同一个 ClaimEvent。建议事件结构允许一个 event 内含多个 claim line。前端 diff 要能显示这条消息同时改变了多个商品。


十一、模拟模式下的 LLM 调用策略

模拟排谷时，有两种模式：真实 LLM 解析和固定解析结果。

真实 LLM 模式适合测试系统实际运行效果，但不可完全复现，因为模型可能更新或温度采样有差异。

固定解析模式适合争议复盘和单元测试。做法是把每条 RawMessage 对应的 ParsedEvent 存下来，之后模拟时不再调用 LLM。

建议支持 ParserMode：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParserMode {
    LiveLlm,
    CachedParse { cache_path: String },
    RuleOnly,
    HybridCachedThenLlm { cache_path: String },
}
```

解析缓存格式：

```json
{
  "parser_version": "parser_prompt_2026_05_08_a",
  "records": [
    {
      "raw_message_id": "m1",
      "input_hash": "sha256:xxx",
      "parsed_event": {
        "intent": "claim",
        "items": [
          {"name":"燐音", "quantity":2, "claim_type":"split"}
        ]
      }
    }
  ]
}
```

input_hash 必须校验。如果消息文本变了但 message_id 没变，不能直接用旧缓存。

Rust 伪代码：

```rust
pub async fn parse_with_mode(
    mode: &ParserMode,
    llm_parser: &dyn MessageParser,
    cache: &mut ParseCache,
    round: &RoundConfig,
    raw: &RawMessage,
) -> Result<ParsedMessage, ParseError> {
    match mode {
        ParserMode::LiveLlm => llm_parser.parse(round, raw).await,
        ParserMode::CachedParse { .. } => cache.get(raw).ok_or(ParseError::CacheMiss),
        ParserMode::RuleOnly => RuleBasedParser::parse(round, raw),
        ParserMode::HybridCachedThenLlm { .. } => {
            if let Some(parsed) = cache.get(raw) {
                Ok(parsed)
            } else {
                let parsed = llm_parser.parse(round, raw).await?;
                cache.insert(raw, &parsed)?;
                Ok(parsed)
            }
        }
    }
}
```


十二、前端 Replay API 设计

后端提供以下 API 即可支撑前端图形化重放。

GET /api/rounds/{round_id}/replays
返回该团所有 replay。

GET /api/rounds/{round_id}/replays/{replay_id}/manifest
返回 manifest。

GET /api/rounds/{round_id}/replays/{replay_id}/steps?from=0&limit=100
返回 step 简表，用于左侧时间轴。

GET /api/rounds/{round_id}/replays/{replay_id}/steps/{step_index}
返回完整 ReplayStep。

GET /api/rounds/{round_id}/replays/{replay_id}/snapshots/{step_index}
返回某一步后的完整 AllocationSnapshot。

GET /api/rounds/{round_id}/replays/{replay_id}/diff/{step_index}
返回某一步的 StateDiff。

POST /api/simulations
提交一个模拟任务。如果你不做异步任务，也可以直接返回结果。由于本系统可以本地跑，也可以命令行跑，这个 API 不是必须。

前端播放不需要一次性拉完整 replay。建议先拉 manifest 和 step 简表，用户点击某一步时再拉详情。


十三、前端 step 简表结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStepSummary {
    pub step_index: u64,
    pub event_id: String,
    pub raw_message_id: Option<String>,
    pub occurred_at_ms: i64,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub event_kind: String,
    pub summary: String,
    pub status: ReplayStepStatus,
    pub changed_slot_count: u32,
    pub warning_count: u32,
    pub error_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplayStepStatus {
    Applied,
    PartiallyApplied,
    Ignored,
    ParseFailed,
    ValidationFailed,
    Error,
}
```

summary 示例：

甲：排 燐音吧唧 x2
乙：撤销 燐音吧唧 x1
管理员：锁定 box2 末尾段给 丙
系统：应用满 800-120 平台券


十四、图形化 Diff 需要区分“直接变化”和“连锁变化”

撤销会导致连锁前移。前端如果只显示最后结果，用户会很难理解。StateDiff 应该区分 DirectChange 和 CascadeChange。

例如：

甲撤销 slot2。
乙从 slot3 前移到 slot2。
丙从 slot4 前移到 slot3。
slot4 变空。

数据结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeCausality {
    Direct,
    Cascade { caused_by_slot: SlotRef },
    Recalculation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotChange {
    pub item_id: String,
    pub box_index: u32,
    pub slot_index: u32,
    pub before: Option<SlotView>,
    pub after: Option<SlotView>,
    pub reason: SlotChangeReason,
    pub causality: ChangeCausality,
}
```

前端显示时，可以把直接变化放在最上面，连锁变化折叠显示。这样争议排查会清楚很多。


十五、包尾锁位在 Replay Viewer 中的展示

包尾最容易误解，所以必须可视化。

建议每个 SlotView 加字段：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotView {
    pub status: SlotStatus,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub claim_id: Option<String>,
    pub segment_id: Option<String>,
    pub slot_policy: SlotPolicy,
    pub lock_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlotStatus {
    Empty,
    Filled,
    Reserved,
    Locked,
    AdminFixed,
}
```

包尾段在 UI 中要显示为一个连续区块。例如：

box1：A | B | 空 | 空 | 空
box2：包尾-丙 | 包尾-丙 | 预留-丙 | 预留-丙 | 空

点击包尾段时，右侧解释：

该段由消息 m17 创建。
策略：tail_locked。
该段不自动回填 box1 空位。
该段不阻挡后续散户填入 box1 空位。
该段可被用户撤销或管理员解锁。


十六、错误事件也要进入时间轴

不要把解析失败、校验失败的消息直接丢掉。它们应该进入 Replay Timeline，但状态为 ParseFailed 或 ValidationFailed。否则用户会问“我明明发了，为什么没有”。

建议定义 FailedReplayStep：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedReplayStep {
    pub replay_id: String,
    pub step_index: u64,
    pub raw_message: RawMessage,
    pub failure_stage: FailureStage,
    pub error_code: String,
    pub error_message: String,
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailureStage {
    Parse,
    Validation,
    Allocation,
    Settlement,
}
```

示例：

原始消息：排燐音2
失败原因：当前同时存在“燐音吧唧”和“燐音色纸”，无法确定商品。
建议修正：请回复“确认 吧唧”或重新发送“排燐音吧唧2”。


十七、模拟报告内容

SimulationReport 应该不仅告诉你最终结果，还要告诉你哪些消息有问题。

建议字段：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationReport {
    pub replay_id: String,
    pub round_id: String,
    pub input_message_count: u64,
    pub parsed_event_count: u64,
    pub applied_event_count: u64,
    pub parse_failure_count: u64,
    pub validation_failure_count: u64,
    pub allocation_warning_count: u64,
    pub final_allocated_claim_count: u64,
    pub final_unallocated_claim_count: u64,
    pub final_total_amount: i64,
    pub manifest_path: String,
    pub final_snapshot_path: String,
    pub failed_messages_path: String,
    pub warnings_path: String,
}
```

输出文件建议：

out/replay_es_2026_05/manifest.json
out/replay_es_2026_05/final_snapshot.json
out/replay_es_2026_05/final_settlement.json
out/replay_es_2026_05/steps.jsonl
out/replay_es_2026_05/failed_messages.jsonl
out/replay_es_2026_05/warnings.jsonl
out/replay_es_2026_05/report.json


十八、可复现性要求

为了做到“同样输入得到同样输出”，Replay Manifest 必须记录以下信息：

1. round_config_hash：团配置哈希。
2. item_config_hash：商品和别名配置哈希。
3. eligibility_hash：优先权表哈希。
4. settlement_rule_hash：结算规则哈希。
5. parser_version：解析器版本。
6. parser_prompt_hash：LLM prompt 哈希。
7. model_name：使用的模型名。
8. parser_mode：LiveLlm / CachedParse / RuleOnly。
9. allocation_engine_version：排队引擎版本。
10. settlement_engine_version：结算引擎版本。
11. input_queue_hash：消息队列文件哈希。

哈希计算建议对规范化 JSON 做 SHA256。

```rust
pub fn stable_json_sha256<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let json = serde_json::to_string(value)?;
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(json.as_bytes());
    Ok(format!("sha256:{:x}", hasher.finalize()))
}
```

如果两次 replay 的 manifest 中这些 hash 完全一致，则最终结果必须一致。如果不一致，前端应该提示“本次回放使用了不同配置或不同解析器，不可直接作为同一版本复盘”。


十九、测试用例设计

事件重放系统必须配套测试，否则容易变成“看起来很美”的日志系统。

建议至少写这些测试：

1. 普通手速排序：同商品多人排，按时间戳分配。
2. 优先权排序：优先用户时间较晚但仍排在前面。
3. 优先权过期：优先用户超过有效期后不插队。
4. 撤销前移：中间用户撤销，后续普通用户自动前移。
5. 包尾锁位：包尾用户不填前面空格，也不挡后续散户填前面空格。
6. 管理员固定：admin_fixed 槽位不参与自动重排。
7. 多商品混合：一条消息同时修改多个商品。
8. 解析失败入时间轴：失败消息不影响最终表，但在 timeline 可见。
9. 结算规则变更：结算 diff 能显示用户金额变化。
10. CachedParse 可复现：同一 queue 和 parse cache 结果完全一致。
11. LiveLlm 不可复现提示：manifest 标记为 weak reproducibility。
12. Snapshot restore：从第 50 步 full snapshot 加 diff 还原第 73 步，与直接 replay 到第 73 步一致。

测试伪代码：

```rust
#[tokio::test]
async fn cancel_should_move_normal_slots_forward() {
    let round = fixture_round_with_one_item_box_size_5();
    let events = vec![
        claim("u1", "item_a", 1, 1000),
        claim("u2", "item_a", 1, 1001),
        claim("u3", "item_a", 1, 1002),
        cancel("u2", "item_a", 1, 1003),
    ];

    let result = replay(round, events).await.unwrap();
    let final_snapshot = result.final_snapshot;
    let slots = final_snapshot.item("item_a").box_at(0).slots();

    assert_eq!(slots[0].user_id(), Some("u1"));
    assert_eq!(slots[1].user_id(), Some("u3"));
    assert!(slots[2].is_empty());

    let step = result.steps.last().unwrap();
    assert!(step.state_diff.slot_changes.iter().any(|c| {
        c.reason == SlotChangeReason::AutoMovedForward
    }));
}
```


二十、前端播放算法

前端有两种播放方式。

方式一：直接请求每一步完整 snapshot。实现最简单，但网络流量大。

方式二：加载最近 full snapshot，然后应用 diff。性能更好，但前端逻辑复杂。

建议第一版用方式一。因为排谷团规模通常不会大到不可承受，而且调试系统优先级是正确性和可理解性，不是极致性能。后续再优化为 full snapshot + diff。

前端状态：

```ts
interface ReplayViewerState {
  manifest: ReplayManifest | null;
  stepSummaries: ReplayStepSummary[];
  currentStepIndex: number;
  currentStep: ReplayStep | null;
  currentSnapshot: AllocationSnapshot | null;
  isPlaying: boolean;
  speed: number;
  filters: ReplayFilters;
}
```

播放伪代码：

```ts
async function goToStep(stepIndex: number) {
  const step = await fetchStep(stepIndex);
  const snapshot = await fetchSnapshot(stepIndex);
  state.currentStepIndex = stepIndex;
  state.currentStep = step;
  state.currentSnapshot = snapshot;
  highlightDiff(step.state_diff);
}

async function play() {
  state.isPlaying = true;
  while (state.isPlaying && state.currentStepIndex < state.stepSummaries.length - 1) {
    await goToStep(state.currentStepIndex + 1);
    await sleep(1000 / state.speed);
  }
}
```


二十一、审计视图里的“人类可读解释”模板

后端可以生成结构化字段，前端负责渲染成人话。不要把所有解释都写死在后端字符串里。

模板示例：

Claim 成功：

“{nickname} 在 {time} 发送消息：{raw_text}。系统识别为排 {item_name} x {quantity}。该用户优先级为 {priority_level}，排序键为 {sort_key}。最终填入 {slot_refs}。”

撤销成功：

“{nickname} 撤销 {item_name} x {quantity}。释放 {released_slots}。因普通槽位允许自动前移，后续 {moved_count} 个槽位发生连锁移动。”

包尾锁位：

“{nickname} 创建包尾锁位段 {segment_id}。该段不会自动填补前方空位，也不会阻挡后续普通请求填补前方空位。”

解析失败：

“系统无法唯一识别该消息中的商品：{ambiguous_text}。候选商品为 {candidate_items}。该消息未进入排队。”


二十二、管理员回放修正能力

回放系统还可以用于修正错误。建议前端支持“从某一步创建修正事件”。例如某条消息被 LLM 解析错了，管理员可以点击该 step，选择“创建修正事件”，系统生成 AdminAdjust 或 ParseOverride。

不要直接改历史事件。正确方式是追加修正事件。

新增事件类型：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseOverrideEvent {
    pub event_id: String,
    pub round_id: String,
    pub target_raw_message_id: String,
    pub corrected_parsed_message: ParsedMessage,
    pub admin_user_id: String,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}
```

修正后重新 replay，得到新 replay_id。旧 replay 保留。这样可以比较：修正前和修正后差异。

前端可以提供 Replay Compare：

左边 replay_old，右边 replay_new，显示最终差异和关键 step 差异。


二十三、数据库表补充

如果使用 PostgreSQL，建议补充以下表。

replay_runs：

```sql
CREATE TABLE replay_runs (
    replay_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL,
    input_source TEXT NOT NULL,
    parser_mode TEXT NOT NULL,
    manifest_json JSONB NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);
```

replay_steps：

```sql
CREATE TABLE replay_steps (
    replay_id TEXT NOT NULL,
    step_index BIGINT NOT NULL,
    round_id TEXT NOT NULL,
    event_id TEXT,
    raw_message_id TEXT,
    occurred_at TIMESTAMPTZ,
    step_summary JSONB NOT NULL,
    state_diff JSONB,
    decision_trace JSONB,
    warnings JSONB NOT NULL DEFAULT '[]'::jsonb,
    errors JSONB NOT NULL DEFAULT '[]'::jsonb,
    PRIMARY KEY (replay_id, step_index)
);
```

replay_snapshots：

```sql
CREATE TABLE replay_snapshots (
    replay_id TEXT NOT NULL,
    step_index BIGINT NOT NULL,
    round_id TEXT NOT NULL,
    snapshot_kind TEXT NOT NULL,
    allocation_snapshot JSONB,
    settlement_snapshot JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (replay_id, step_index)
);
```

parse_overrides：

```sql
CREATE TABLE parse_overrides (
    override_id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL,
    raw_message_id TEXT NOT NULL,
    corrected_parsed_message JSONB NOT NULL,
    admin_user_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

如果你希望继续使用 R2 为主，也可以只把 replay_runs 和索引信息放数据库，完整 step 存 R2。


二十四、API 安全与隐私

回放数据包含用户 QQ、昵称、发言内容、购买偏好和金额，不能公开暴露。前端静态页面如果通过 R2 直接读 current.json，需要注意权限。

建议：

1. 公开展示页只显示脱敏昵称和排位，不显示金额。
2. 管理员审计页需要登录。
3. ReplayStep 里的 raw_text 只对管理员可见。
4. 导出的争议截图可以隐藏 user_id，只显示群昵称。
5. R2 对象如果是公开桶，不要存完整 raw message；完整审计数据走后端签名 URL 或私有 API。

脱敏结构：

```rust
pub struct PublicSlotView {
    pub display_name: String,
    pub slot_policy: SlotPolicy,
    pub status: SlotStatus,
}

pub struct AdminSlotView {
    pub user_id: String,
    pub nickname: String,
    pub raw_message_id: Option<String>,
    pub claim_id: Option<String>,
    pub slot_policy: SlotPolicy,
    pub status: SlotStatus,
}
```


二十五、和原架构的关系

这份补充文档不要求推翻原设计。需要新增的是：

1. 原始消息必须保留，并且有稳定 source_sequence。
2. ValidatedEvent 必须有稳定 order_key。
3. Allocation Engine 必须支持 with_trace 版本。
4. Snapshot Publisher 除 current.json 外，还要能输出 replay timeline。
5. Parser 必须支持 cache，以便模拟回放可复现。
6. 前端除 current table 外，增加 replay viewer。
7. CLI 增加 simulate 命令，读取消息队列文件。

最终系统应有两种入口（加上 WS 共三种）：

实时入口（WS）：QQ WS → WS Server (port 3001) → IncomingQqMessage → Parser → Validator → EventStore → 当前状态 → current.json

实时入口（Webhook）：POST /webhook/qq-message → IncomingQqMessage → Parser → Validator → EventStore → 当前状态 → current.json

模拟入口：queue.jsonl → RawMessage → Parser 或 ParseCache → Validator → ReplayEngine → replay timeline → final report

两种入口在 Validator 之后共用同一个事件模型，在 Allocation Engine 处共用同一套排队规则。


二十六、最重要的实现建议

不要把 Replay Engine 写成“读取历史快照并播放”。那只是播放器，不是审计系统。真正有价值的 Replay Engine 必须从事件重新计算状态。

不要让前端自己推导业务规则。前端只展示后端产出的 snapshot、diff 和 trace。

不要覆盖历史记录。任何修正都追加新事件，然后生成新的 replay_id。

不要只记录成功事件。失败消息同样需要进入时间轴，否则用户无法确认系统是否看见了自己的消息。

不要让 LLM 结果成为不可解释的黑箱。每条 LLM 解析都要保留 raw_text、parser_version、prompt_hash、model_name、parsed_json、confidence、ambiguous_parts。

这套补充设计完成后，系统不仅能排谷，还能回答“为什么这样排”。这对于手速团、优先权、包尾、撤销和优惠分摊都很关键。
