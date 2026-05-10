# QQ机器人 + LLM 排谷系统 Rust 后端架构与计划文档

本文档面向“由 LLM 一次性生成主要代码”的使用场景，因此会尽量把模块边界、数据结构、关键算法、状态机、错误处理、接口契约、伪代码和 Rust 结构都写清楚。消息收发层已有现成框架，因此本文不设计 WebSocket 连接与 QQ 消息网关本身，只定义消息进入本系统后的处理方式，以及系统应输出什么样的回复、快照和导出数据。

系统目标：管理员配置一场或多场拼团；群成员在指定时间通过自然语言排谷、撤销、修改；系统用 LLM 解析自然语言，但用确定性业务引擎执行排队、锁位、撤销、优惠分摊和账单结算；前端通过静态页面读取快照实时展示；结团时输出商品维度、用户维度、订单维度的表格数据。

一、总体设计原则

1. 不直接改最终表格。所有输入都先保存成事件，最终状态由事件流重放得到。

2. LLM 只负责“语言到结构化候选事件”的解析，不负责排位、不负责价格、不负责优惠计算、不负责权限判断。

3. 所有业务判断必须可复现、可审计、可重放。任意时刻都可以通过 round_id 读取全部事件并重建当前状态。

4. 用户消息、解析结果、校验结果、业务事件、排队快照、结算快照必须分层保存。

5. 普通拼团、单领、包尾端盒、锁列、管理员手动修正、撤销、优惠、赠品等规则都要作为明确的数据模型存在，不要靠字符串备注表达。

6. 多个团同时运行时，必须以 round_id 作为强隔离边界。LLM 可以辅助推断 round_id，但程序必须校验；如果无法唯一确定，则进入待确认状态。

7. 金额必须使用整数分为单位，例如 45 元存为 4500，不使用 f32/f64。

8. 时间必须使用带时区的时间类型，统一内部使用 UTC 时间戳，展示时转换为 Asia/Shanghai 或 Asia/Singapore，具体按部署配置。

二、推荐技术栈

语言：Rust。

异步运行时：tokio。

HTTP API：axum。

序列化：serde、serde_json。

数据库：PostgreSQL，Rust 访问层推荐 sqlx。

缓存可选：Redis。第一版可以不用 Redis。

对象存储：Cloudflare R2，使用 S3 兼容 API。Rust SDK 可用 aws-sdk-s3。

LLM 调用：抽象成 trait，不绑定具体供应商。实现可以是 OpenAI 兼容接口、本地模型、或其他服务。

配置：figment 或 config crate，也可以先用 envy + 环境变量。

日志：tracing、tracing-subscriber。

错误：thiserror、anyhow。业务层建议 thiserror 定义明确错误。

十进制金额：内部用 i64 cents，不建议引入 Decimal，除非后续出现比例复杂分摊。

三、工程目录结构

建议项目名为 paigu-bot-core。

```text
paigu-bot-core/
  Cargo.toml
  src/
    main.rs
    config.rs
    error.rs
    app_state.rs

    domain/
      mod.rs
      ids.rs
      money.rs
      time.rs
      user.rs
      round.rs
      item.rs
      claim.rs
      event.rs
      allocation.rs
      settlement.rs
      snapshot.rs
      export.rs

    inbound/
      mod.rs
      qq_message.rs
      intake.rs
      command_router.rs

    parser/
      mod.rs
      llm_client.rs
      prompt.rs
      parsed_event.rs
      normalize.rs
      alias_match.rs
      validation.rs

    engine/
      mod.rs
      event_store.rs
      replay.rs
      allocation_engine.rs
      slot_allocator.rs
      settlement_engine.rs
      discount_engine.rs
      gift_engine.rs
      priority.rs
      policy.rs

    repo/
      mod.rs
      postgres.rs
      raw_message_repo.rs
      round_repo.rs
      item_repo.rs
      event_repo.rs
      snapshot_repo.rs
      user_repo.rs
      eligibility_repo.rs

    services/
      mod.rs
      message_service.rs
      round_service.rs
      admin_service.rs
      claim_service.rs
      cancel_service.rs
      settlement_service.rs
      snapshot_service.rs
      export_service.rs

    api/
      mod.rs
      routes.rs
      admin_routes.rs
      public_routes.rs
      webhook_routes.rs

    publisher/
      mod.rs
      r2_publisher.rs
      local_publisher.rs

    tests/
      mod.rs

    ws/
      mod.rs
      ws_server.rs
```

四、核心流程

系统收到 QQ 框架传入的消息后，执行以下链路（两种入口：WebSocket 或 HTTP Webhook）：

```text
QQ框架消息
→ WS Server (port 3001) 或 Webhook API (POST /webhook/qq-message)
→ IncomingQqMessage 统一消息体
→ RawMessage 入库（幂等校验：group_id+qq_message_id 唯一）
→ command_router 判断消息类型
→ 管理员命令或成员排谷消息
→ LLM Parser 生成 ParsedEvent
→ Validation Layer 校验 round、item、user、权限、时间窗口、歧义
→ EventStore 写入 ValidatedEvent
→ Replay 当前 round 事件
→ AllocationEngine 生成 AllocationSnapshot
→ SettlementEngine 生成 SettlementSnapshot
→ SnapshotPublisher 写 current.json 到 R2
→ 返回 BotReply（WS 返回 JSON，Webhook 返回 HTTP JSON），例如"已记录""需确认""解析失败""已撤销"
```

五、数据库表设计

以下是逻辑表。实际可以用 SQL migration 建表。

1. users

保存群成员基础信息。

```sql
CREATE TABLE users (
  user_id TEXT PRIMARY KEY,
  qq_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
```

2. groups

```sql
CREATE TABLE groups (
  group_id TEXT PRIMARY KEY,
  name TEXT,
  created_at TIMESTAMPTZ NOT NULL
);
```

3. rounds

一场拼团。

```sql
CREATE TABLE rounds (
  round_id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  start_at TIMESTAMPTZ,
  end_at TIMESTAMPTZ,
  allow_cancel BOOLEAN NOT NULL DEFAULT TRUE,
  allow_modify BOOLEAN NOT NULL DEFAULT TRUE,
  default_timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
  created_by TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
```

status 可选：draft、scheduled、active、settling、closed、archived。

4. items

商品表。拼团商品和单领商品都在这里。

```sql
CREATE TABLE items (
  item_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  name TEXT NOT NULL,
  item_kind TEXT NOT NULL,
  unit_price_cents BIGINT NOT NULL,
  box_size INT,
  max_quantity INT,
  is_blind BOOLEAN NOT NULL DEFAULT FALSE,
  is_proxy_card BOOLEAN NOT NULL DEFAULT FALSE,
  sort_order INT NOT NULL DEFAULT 0,
  metadata JSONB NOT NULL DEFAULT '{}',
  created_at TIMESTAMPTZ NOT NULL
);
```

item_kind 可选：split、single、gift、shipping、adjustment。

5. item_aliases

```sql
CREATE TABLE item_aliases (
  alias_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  item_id TEXT NOT NULL,
  alias TEXT NOT NULL,
  weight INT NOT NULL DEFAULT 100
);
```

6. eligibility

优先权表。

```sql
CREATE TABLE eligibility (
  eligibility_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  priority_type TEXT NOT NULL,
  priority_level INT NOT NULL,
  scope JSONB NOT NULL DEFAULT '{}',
  max_uses INT,
  used_count INT NOT NULL DEFAULT 0,
  valid_from TIMESTAMPTZ,
  valid_until TIMESTAMPTZ,
  note TEXT,
  created_at TIMESTAMPTZ NOT NULL
);
```

scope 示例：

```json
{
  "item_ids": ["badge_rinne", "card_himeru"],
  "item_kinds": ["split"],
  "only_before_start_minutes": 5
}
```

7. raw_messages

```sql
CREATE TABLE raw_messages (
  raw_message_id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  qq_message_id TEXT NOT NULL,
  timestamp TIMESTAMPTZ NOT NULL,
  text TEXT,
  images JSONB NOT NULL DEFAULT '[]',
  is_admin BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL,
  UNIQUE(group_id, qq_message_id)
);
```

8. parsed_messages

保存 LLM 原始输出和解析后结构，方便排错。

```sql
CREATE TABLE parsed_messages (
  parsed_message_id TEXT PRIMARY KEY,
  raw_message_id TEXT NOT NULL,
  parser_version TEXT NOT NULL,
  prompt_hash TEXT NOT NULL,
  llm_raw_response TEXT NOT NULL,
  parsed_json JSONB NOT NULL,
  confidence NUMERIC,
  status TEXT NOT NULL,
  error TEXT,
  created_at TIMESTAMPTZ NOT NULL
);
```

status 可选：ok、ambiguous、failed、ignored。

9. events

所有业务事件。

```sql
CREATE TABLE events (
  event_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  group_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  raw_message_id TEXT,
  event_type TEXT NOT NULL,
  effective_at TIMESTAMPTZ NOT NULL,
  sequence BIGSERIAL NOT NULL,
  payload JSONB NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TIMESTAMPTZ NOT NULL
);
```

event_type 示例：claim、cancel_claim、modify_claim、admin_create_round、admin_add_item、admin_adjust_allocation、admin_lock_slot、admin_set_discount_rule、admin_close_round、payment_mark。

sequence 用数据库自增，解决同一时间戳消息的稳定排序。

10. snapshots

```sql
CREATE TABLE snapshots (
  snapshot_id TEXT PRIMARY KEY,
  round_id TEXT NOT NULL,
  version BIGINT NOT NULL,
  allocation_json JSONB NOT NULL,
  settlement_json JSONB NOT NULL,
  public_json JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  UNIQUE(round_id, version)
);
```

六、Rust 核心类型定义

1. ID 类型

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoundId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClaimId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub String);
```

2. 金额类型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MoneyCents(pub i64);

impl MoneyCents {
    pub fn zero() -> Self { Self(0) }
    pub fn from_yuan(yuan: i64) -> Self { Self(yuan * 100) }
    pub fn checked_add(self, rhs: Self) -> anyhow::Result<Self> {
        self.0.checked_add(rhs.0).map(Self).ok_or_else(|| anyhow::anyhow!("money overflow"))
    }
    pub fn checked_mul_i64(self, n: i64) -> anyhow::Result<Self> {
        self.0.checked_mul(n).map(Self).ok_or_else(|| anyhow::anyhow!("money overflow"))
    }
}
```

3. Round

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundStatus {
    Draft,
    Scheduled,
    Active,
    Settling,
    Closed,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    pub round_id: RoundId,
    pub group_id: String,
    pub title: String,
    pub status: RoundStatus,
    pub start_at: Option<chrono::DateTime<chrono::Utc>>,
    pub end_at: Option<chrono::DateTime<chrono::Utc>>,
    pub allow_cancel: bool,
    pub allow_modify: bool,
    pub default_timezone: String,
}
```

4. Item

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemKind {
    Split,
    Single,
    Gift,
    Shipping,
    Adjustment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub item_id: ItemId,
    pub round_id: RoundId,
    pub name: String,
    pub kind: ItemKind,
    pub unit_price: MoneyCents,
    pub box_size: Option<u32>,
    pub max_quantity: Option<u32>,
    pub is_blind: bool,
    pub is_proxy_card: bool,
    pub aliases: Vec<String>,
    pub sort_order: i32,
}
```

5. ParsedEvent

这是 LLM 输出后进入程序校验前的结构。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParsedIntent {
    Claim,
    Cancel,
    Modify,
    ConfirmAmbiguous,
    AdminCommand,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMessage {
    pub intent: ParsedIntent,
    pub round_hint: Option<String>,
    pub items: Vec<ParsedClaimItem>,
    pub cancel_target_hint: Option<String>,
    pub admin_command: Option<ParsedAdminCommand>,
    pub confidence: f32,
    pub ambiguous_parts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedClaimItem {
    pub name: String,
    pub category_hint: Option<String>,
    pub quantity: u32,
    pub claim_type: Option<ClaimType>,
    pub is_proxy_card: Option<bool>,
    pub slot_policy: Option<SlotPolicy>,
    pub notes: Option<String>,
}
```

6. ClaimType 与 SlotPolicy

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimType {
    Split,
    Single,
    GiftClaim,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotPolicy {
    Normal,
    TailLocked,
    ColumnLocked,
    AdminFixed,
}
```

7. ValidatedEvent

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    ClaimCreated(ClaimCreated),
    ClaimCancelled(ClaimCancelled),
    ClaimModified(ClaimModified),
    AdminAllocationAdjusted(AdminAllocationAdjusted),
    AdminSlotLocked(AdminSlotLocked),
    DiscountRulesSet(DiscountRulesSet),
    RoundClosed(RoundClosed),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: EventId,
    pub round_id: RoundId,
    pub group_id: String,
    pub user_id: UserId,
    pub raw_message_id: Option<String>,
    pub event_type: String,
    pub effective_at: chrono::DateTime<chrono::Utc>,
    pub sequence: i64,
    pub payload: DomainEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimCreated {
    pub claim_id: ClaimId,
    pub user_id: UserId,
    pub items: Vec<ClaimLine>,
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimLine {
    pub item_id: ItemId,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub slot_policy: SlotPolicy,
    pub is_proxy_card: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimCancelled {
    pub target_claim_id: Option<ClaimId>,
    pub target_item_id: Option<ItemId>,
    pub quantity: Option<u32>,
    pub reason: Option<String>,
}
```

七、LLM 解析契约

LLM 输入应该包含：当前群、发言人、消息文本、活跃 round 列表、每个 round 的商品名和 alias、当前时间、允许的命令格式、示例。

LLM 输出必须是 JSON，不允许输出解释文字。程序应使用 JSON schema 或强类型反序列化校验。

系统提示词核心内容：

```text
你是排谷系统的自然语言解析器。你的任务是把用户消息解析成 JSON。不要判断能不能排上，不要计算价格，不要生成回复。只抽取意图、商品名、数量、拼团/单领、是否代牌、是否包尾/端盒/锁列、撤销对象、管理员命令。若不确定，填写 ambiguous_parts。输出必须是合法 JSON。
```

用户消息上下文示例：

```json
{
  "group_id": "123",
  "user_id": "456",
  "nickname": "A",
  "message": "排燐音吧唧2，蓝良单领1，特典包尾",
  "active_rounds": [
    {
      "round_id": "es_2026_05_badge",
      "title": "ES 5月吧唧团",
      "items": [
        {"item_id":"badge_rinne", "name":"燐音吧唧", "aliases":["燐音", "rinne", "天城燐音"], "kind":"split"},
        {"item_id":"badge_aira", "name":"蓝良吧唧", "aliases":["蓝良", "aira"], "kind":"split"}
      ]
    }
  ]
}
```

LLM 输出示例：

```json
{
  "intent": "Claim",
  "round_hint": "ES 5月吧唧团",
  "items": [
    {
      "name": "燐音吧唧",
      "category_hint": "吧唧",
      "quantity": 2,
      "claim_type": "Split",
      "is_proxy_card": false,
      "slot_policy": "Normal",
      "notes": null
    },
    {
      "name": "蓝良",
      "category_hint": null,
      "quantity": 1,
      "claim_type": "Single",
      "is_proxy_card": false,
      "slot_policy": "Normal",
      "notes": null
    },
    {
      "name": "特典",
      "category_hint": null,
      "quantity": 1,
      "claim_type": "GiftClaim",
      "is_proxy_card": false,
      "slot_policy": "TailLocked",
      "notes": "包尾"
    }
  ],
  "cancel_target_hint": null,
  "admin_command": null,
  "confidence": 0.86,
  "ambiguous_parts": []
}
```

八、LLM 调用接口

```rust
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    async fn parse_message(&self, req: LlmParseRequest) -> Result<LlmParseResponse, LlmError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmParseRequest {
    pub system_prompt: String,
    pub user_payload: serde_json::Value,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmParseResponse {
    pub raw_text: String,
    pub parsed: ParsedMessage,
    pub model: String,
}
```

LLM 解析失败的策略：

1. JSON 反序列化失败：保存 parsed_messages.status=failed，机器人回复“没有识别成功，请按格式重发”。

2. confidence 低于阈值，例如 0.65：进入 ambiguous，机器人回复候选项或要求重发。

3. 商品匹配多个：不落业务事件，保存待确认状态。

4. 商品匹配不到：回复当前可排商品列表的简略版。

九、商品别名匹配

LLM 输出 name 之后，程序必须用 alias 做确定性匹配。

匹配顺序：

1. item_id 精确匹配。

2. alias 精确匹配。

3. 商品名完全包含。

4. alias + category_hint 联合匹配。

5. 模糊匹配。模糊匹配只作为候选，不自动落库，除非唯一且分数高于阈值。

伪代码：

```rust
pub fn resolve_item(
    parsed: &ParsedClaimItem,
    rounds: &[RoundContext],
) -> ResolveResult {
    let mut candidates = Vec::new();

    for round in rounds {
        for item in &round.items {
            let mut score = 0;
            if item.item_id.0 == parsed.name { score += 1000; }
            if item.name == parsed.name { score += 900; }
            if item.aliases.iter().any(|a| a == &parsed.name) { score += 800; }
            if item.name.contains(&parsed.name) { score += 400; }
            if let Some(hint) = &parsed.category_hint {
                if item.name.contains(hint) || item.aliases.iter().any(|a| a.contains(hint)) {
                    score += 150;
                }
            }
            if let Some(claim_type) = &parsed.claim_type {
                if item.kind.compatible_with(claim_type) {
                    score += 100;
                } else {
                    score -= 500;
                }
            }
            if score > 0 {
                candidates.push((round.round_id.clone(), item.item_id.clone(), score));
            }
        }
    }

    candidates.sort_by(|a, b| b.2.cmp(&a.2));

    if candidates.is_empty() {
        ResolveResult::NotFound
    } else if candidates.len() == 1 || candidates[0].2 >= candidates[1].2 + 300 {
        ResolveResult::Resolved { round_id: candidates[0].0.clone(), item_id: candidates[0].1.clone() }
    } else {
        ResolveResult::Ambiguous { candidates }
    }
}
```

十、消息服务主流程

```rust
pub struct MessageService {
    raw_repo: Arc<dyn RawMessageRepo>,
    round_repo: Arc<dyn RoundRepo>,
    item_repo: Arc<dyn ItemRepo>,
    parser: Arc<MessageParser>,
    validator: Arc<EventValidator>,
    event_repo: Arc<dyn EventRepo>,
    replay_service: Arc<ReplayService>,
    snapshot_service: Arc<SnapshotService>,
}

impl MessageService {
    pub async fn handle_incoming(&self, msg: IncomingQqMessage) -> Result<BotReply, AppError> {
        let raw = self.raw_repo.insert_raw_message(&msg).await?;

        if msg.is_admin_command_candidate() {
            return self.handle_admin_message(raw, msg).await;
        }

        let active_rounds = self.round_repo.find_active_rounds_by_group(&msg.group_id).await?;
        if active_rounds.is_empty() {
            return Ok(BotReply::silent());
        }

        let round_contexts = self.load_round_contexts(&active_rounds).await?;
        let parsed = self.parser.parse_member_message(&msg, &round_contexts).await?;

        let validated = match self.validator.validate(parsed, &msg, &round_contexts).await? {
            ValidationOutcome::Ok(ev) => ev,
            ValidationOutcome::NeedConfirm(reply) => return Ok(reply),
            ValidationOutcome::Reject(reply) => return Ok(reply),
            ValidationOutcome::Ignore => return Ok(BotReply::silent()),
        };

        self.event_repo.insert_event(&validated).await?;
        let snapshot = self.replay_service.rebuild_round(&validated.round_id).await?;
        self.snapshot_service.save_and_publish(&snapshot).await?;

        Ok(BotReply::text(format!("已记录：{}", snapshot.short_ack_for_user(&msg.user_id))))
    }
}
```

十一、排队引擎总体思路

AllocationEngine 的输入是：round、items、eligibility、active events。输出是 AllocationSnapshot。

不要在事件写入时直接计算局部变化。为了正确处理撤销和前移，建议每次对当前 round 全量重放。只要单场团事件数不是几万级，全量重放足够稳定。后续可做增量优化。

事件排序：

```text
priority_level DESC
claim.effective_at ASC
event.sequence ASC
line_index ASC
```

注意：priority_level 是 claim line 维度计算出来的，不是 event 全局。因为某人的优先权可能只对某些 item 生效。

十二、排队相关数据结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationSnapshot {
    pub round_id: RoundId,
    pub version: i64,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub item_allocations: Vec<ItemAllocation>,
    pub user_summaries: Vec<UserAllocationSummary>,
    pub warnings: Vec<AllocationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAllocation {
    pub item_id: ItemId,
    pub item_name: String,
    pub kind: ItemKind,
    pub boxes: Vec<BoxAllocation>,
    pub singles: Vec<SingleAllocation>,
    pub waiting: Vec<WaitingLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxAllocation {
    pub box_index: u32,
    pub slots: Vec<SlotAllocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotAllocation {
    pub slot_index: u32,
    pub user_id: Option<UserId>,
    pub claim_id: Option<ClaimId>,
    pub claim_line_index: Option<u32>,
    pub status: SlotStatus,
    pub slot_policy: SlotPolicy,
    pub segment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SlotStatus {
    Empty,
    Filled,
    LockedEmpty,
    AdminReserved,
}
```

十三、普通拼团分配算法

普通拼团的逻辑是填最早可填空位。box_size 表示一盒几个位置。如果 box 满了，就开下一盒。是否允许未满盒展示取决于团政策；一般实时展示可以显示未满盒，结团时再判断是否成配。

伪代码：

```rust
fn allocate_normal_line(state: &mut ItemWorkingState, line: &EffectiveClaimLine) {
    for _ in 0..line.quantity {
        let slot = state.find_first_fillable_normal_slot();
        match slot {
            Some((box_idx, slot_idx)) => {
                state.fill_slot(box_idx, slot_idx, line, SlotPolicy::Normal, None);
            }
            None => {
                let (box_idx, slot_idx) = state.create_next_box_and_first_slot();
                state.fill_slot(box_idx, slot_idx, line, SlotPolicy::Normal, None);
            }
        }
    }
}
```

可填空位必须满足：

```text
status == Empty
slot_policy == Normal
不属于 TailLocked segment
不属于 AdminFixed / AdminReserved
```

十四、包尾 / 端盒 / 锁列算法

包尾不是普通数量，而是生成一个独立 segment。这个 segment 不补前面的空位，也不阻塞后来的普通散户填前面的空位。

系统需要支持两类常见语义：

1. 包尾当前盒：从当前最后一个盒子的某个位置开始锁定。

2. 端盒另开：直接新开一个盒或列作为该用户的锁定区域。

推荐第一版统一解释为“端盒另开”，即 TailLocked 总是创建新的 segment，不回填旧空格。这样最符合你提到的“不自动归并到之前仍然有空位的顺序排”。

伪代码：

```rust
fn allocate_tail_locked_line(state: &mut ItemWorkingState, line: &EffectiveClaimLine) {
    let segment_id = format!("tail:{}:{}:{}", line.user_id.0, line.claim_id.0, line.line_index);
    let box_idx = state.create_new_box();

    for i in 0..line.quantity {
        let slot_idx = i + 1;
        state.ensure_slot_exists(box_idx, slot_idx);
        state.fill_slot(box_idx, slot_idx, line, SlotPolicy::TailLocked, Some(segment_id.clone()));
    }

    // 如果需要可视化完整盒子，剩余槽位标记为空但属于该 segment，不被普通散户填入。
    if let Some(box_size) = state.box_size {
        for slot_idx in (line.quantity + 1)..=box_size {
            state.ensure_slot_exists(box_idx, slot_idx);
            state.mark_locked_empty(box_idx, slot_idx, SlotPolicy::TailLocked, Some(segment_id.clone()));
        }
    }
}
```

如果之后要支持“只锁一列，不锁整盒”，可把 Segment 加上 shape：

```rust
pub enum SegmentShape {
    FullBox,
    Column { column_index: u32 },
    Range { start_slot: u32, len: u32 },
}
```

十五、撤销算法

撤销不直接删除 allocation。撤销事件参与重放。

重放时先收集 active claims，然后应用 cancel 事件。

撤销目标可以有几种：

1. target_claim_id 明确，撤销整条 claim。

2. target_item_id + quantity，撤销用户最近一次或最早一次该商品的指定数量。

3. 没有目标，撤销该用户最近一条 claim。

建议规则：自然语言“撤刚才”“撤上一条”撤销最近 claim；“撤燐音1”撤销该用户最近的燐音 1 个数量；管理员可以指定 claim_id 或用户。

伪代码：

```rust
fn apply_cancellations(events: &[EventEnvelope]) -> Vec<EffectiveClaim> {
    let mut claims: Vec<EffectiveClaim> = Vec::new();

    for ev in events {
        match &ev.payload {
            DomainEvent::ClaimCreated(c) => claims.push(EffectiveClaim::from(c, ev)),
            DomainEvent::ClaimCancelled(cancel) => {
                apply_cancel(&mut claims, &ev.user_id, cancel);
            }
            DomainEvent::ClaimModified(modify) => {
                apply_modify(&mut claims, &ev.user_id, modify);
            }
            _ => {}
        }
    }

    claims.into_iter().filter(|c| !c.is_empty()).collect()
}

fn apply_cancel(claims: &mut [EffectiveClaim], user_id: &UserId, cancel: &ClaimCancelled) {
    if let Some(target_id) = &cancel.target_claim_id {
        for c in claims.iter_mut() {
            if &c.claim_id == target_id && &c.user_id == user_id {
                c.cancel_all();
            }
        }
        return;
    }

    if let Some(item_id) = &cancel.target_item_id {
        let mut remaining = cancel.quantity.unwrap_or(u32::MAX);
        for c in claims.iter_mut().rev() {
            if &c.user_id == user_id {
                remaining = c.cancel_item_quantity(item_id, remaining);
                if remaining == 0 { break; }
            }
        }
        return;
    }

    for c in claims.iter_mut().rev() {
        if &c.user_id == user_id && !c.is_empty() {
            c.cancel_all();
            break;
        }
    }
}
```

十六、优先权排序算法

Eligibility 的优先权应作用于 claim line，而不是整个用户。

```rust
pub struct EffectiveClaimLine {
    pub claim_id: ClaimId,
    pub line_index: u32,
    pub user_id: UserId,
    pub item_id: ItemId,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub slot_policy: SlotPolicy,
    pub effective_at: DateTime<Utc>,
    pub sequence: i64,
    pub priority_level: i32,
}
```

计算 priority_level：

```rust
fn compute_priority(line: &EffectiveClaimLine, eligibilities: &[Eligibility], now: DateTime<Utc>) -> i32 {
    eligibilities
        .iter()
        .filter(|e| e.user_id == line.user_id)
        .filter(|e| e.applies_to_item(&line.item_id))
        .filter(|e| e.applies_at(line.effective_at))
        .map(|e| e.priority_level)
        .max()
        .unwrap_or(0)
}
```

排序：

```rust
lines.sort_by(|a, b| {
    b.priority_level.cmp(&a.priority_level)
        .then_with(|| a.effective_at.cmp(&b.effective_at))
        .then_with(|| a.sequence.cmp(&b.sequence))
        .then_with(|| a.line_index.cmp(&b.line_index))
});
```

十七、单领商品算法

单领不进入 box slot，而是直接进入用户购买清单。若商品设置 max_quantity，则按优先级排序后截断。

```rust
fn allocate_single_item(item: &Item, lines: Vec<EffectiveClaimLine>) -> Vec<SingleAllocation> {
    let mut result = Vec::new();
    let mut remaining = item.max_quantity.unwrap_or(u32::MAX);

    for line in lines {
        if remaining == 0 {
            // 进入 waiting
            continue;
        }
        let qty = line.quantity.min(remaining);
        result.push(SingleAllocation {
            user_id: line.user_id.clone(),
            claim_id: line.claim_id.clone(),
            item_id: line.item_id.clone(),
            quantity: qty,
            unit_price: item.unit_price,
        });
        remaining -= qty;
    }

    result
}
```

十八、管理员手动修正

管理员修正必须作为事件写入，不能直接改 snapshot。

典型修正：

1. 指定某用户某商品固定到某盒某槽。

2. 锁定某个槽为空，不允许自动填。

3. 解锁某个槽。

4. 将某用户移出某商品。

5. 修改某条 claim 的数量。

管理员修正事件结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminAllocationAdjusted {
    pub adjustment_id: String,
    pub action: AdminAllocationAction,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminAllocationAction {
    FixUserToSlot {
        item_id: ItemId,
        user_id: UserId,
        box_index: u32,
        slot_index: u32,
    },
    LockSlot {
        item_id: ItemId,
        box_index: u32,
        slot_index: u32,
        reason: Option<String>,
    },
    UnlockSlot {
        item_id: ItemId,
        box_index: u32,
        slot_index: u32,
    },
    RemoveUserItem {
        item_id: ItemId,
        user_id: UserId,
        quantity: u32,
    },
}
```

处理顺序建议：先应用普通 claim 排队，再应用管理员修正覆盖。若管理员固定槽与普通自动分配冲突，则普通分配被移出并重新寻找下一个可用槽。为了实现简单，可以在分配前先从事件中构造 admin constraints，然后分配时避开这些槽。

十九、结算引擎

结算引擎输入：AllocationSnapshot、Item 列表、DiscountRule 列表、订单实付信息、赠品规则。

输出：SettlementSnapshot。

结算应分为三个层次：

1. Line charge：每个用户每个商品原价。

2. Discount allocation：满减、购物金、平台券、赠品估值等抵扣如何分摊。

3. Final payable：用户最终应付。

SettlementSnapshot 结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementSnapshot {
    pub round_id: RoundId,
    pub version: i64,
    pub generated_at: DateTime<Utc>,
    pub gross_total: MoneyCents,
    pub discount_total: MoneyCents,
    pub final_total: MoneyCents,
    pub user_bills: Vec<UserBill>,
    pub item_totals: Vec<ItemTotal>,
    pub discount_applications: Vec<DiscountApplication>,
    pub warnings: Vec<SettlementWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBill {
    pub user_id: UserId,
    pub display_name: String,
    pub lines: Vec<UserBillLine>,
    pub gross_total: MoneyCents,
    pub discount_share: MoneyCents,
    pub gift_value_share: MoneyCents,
    pub shipping_fee: MoneyCents,
    pub final_total: MoneyCents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBillLine {
    pub item_id: ItemId,
    pub item_name: String,
    pub kind: ItemKind,
    pub quantity: u32,
    pub unit_price: MoneyCents,
    pub gross: MoneyCents,
}
```

二十、优惠规则类型

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscountRulesSet {
    pub rules: Vec<DiscountRule>,
    pub source_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscountRule {
    ThresholdDiscount {
        rule_id: String,
        scope: DiscountScope,
        threshold: MoneyCents,
        discount: MoneyCents,
        repeatable: bool,
        stackable: bool,
    },
    FixedActualDiscount {
        rule_id: String,
        scope: DiscountScope,
        amount: MoneyCents,
        allocation_policy: DiscountAllocationPolicy,
    },
    ShoppingFund {
        rule_id: String,
        amount: MoneyCents,
        allocation_policy: DiscountAllocationPolicy,
    },
    GiftByThreshold {
        rule_id: String,
        threshold: MoneyCents,
        gift_item_id: ItemId,
        gift_quantity_per_threshold: u32,
        gift_valuation: MoneyCents,
        allocation_policy: GiftAllocationPolicy,
        value_offset_policy: DiscountAllocationPolicy,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscountScope {
    AllPaidItems,
    ItemIds(Vec<ItemId>),
    ItemKinds(Vec<ItemKind>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscountAllocationPolicy {
    ByGrossAmountRatio,
    ByQuantityRatio,
    EqualByUser,
    Manual(Vec<ManualDiscountShare>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GiftAllocationPolicy {
    TreatAsSplitItem,
    GiveToPriorityUsers,
    Manual,
}
```

二十一、优惠分摊算法

最基础且公平的做法是按用户参与优惠范围内的原价金额比例分摊。金额不能有小数，所以用“最大余数法”分摊分。

伪代码：

```rust
fn allocate_discount_by_ratio(total_discount: MoneyCents, user_basis: &[(UserId, MoneyCents)]) -> Vec<(UserId, MoneyCents)> {
    let basis_sum: i64 = user_basis.iter().map(|(_, m)| m.0).sum();
    if basis_sum <= 0 || total_discount.0 <= 0 {
        return user_basis.iter().map(|(u, _)| (u.clone(), MoneyCents::zero())).collect();
    }

    let mut shares = Vec::new();
    let mut allocated = 0i64;

    for (user_id, basis) in user_basis {
        let numerator = total_discount.0 * basis.0;
        let floor = numerator / basis_sum;
        let remainder = numerator % basis_sum;
        allocated += floor;
        shares.push((user_id.clone(), floor, remainder));
    }

    let mut leftover = total_discount.0 - allocated;
    shares.sort_by(|a, b| b.2.cmp(&a.2));

    for share in shares.iter_mut() {
        if leftover <= 0 { break; }
        share.1 += 1;
        leftover -= 1;
    }

    shares.into_iter().map(|(u, cents, _)| (u, MoneyCents(cents))).collect()
}
```

二十二、赠品处理

赠品有两个维度：

1. 物理分配：赠品给谁，是否作为一个拼团商品排位。

2. 财务抵扣：赠品估值用于减少其他商品均价或实付。

建议把赠品也建成 item_kind=Gift 的 Item。GiftByThreshold 规则生成 gift quantity。例如满 500 送 1，当前优惠范围金额为 1280，则赠品数 = floor(1280 / 500) = 2。如果 repeatable 规则不同，可扩展。

如果 allocation_policy=TreatAsSplitItem，则 gift_item_id 对应的 gift 商品也进入 AllocationEngine。问题是赠品数量取决于结算，而结算又依赖 allocation。解决方式：

1. 先根据普通商品 allocation 计算 paid_gross_total。

2. 根据 paid_gross_total 生成 gift item 的 max_quantity 或 available_quantity。

3. 对 gift claim lines 做 allocation。

4. 结算时把 gift_valuation * gift_allocated_quantity 作为负向抵扣，按 value_offset_policy 分摊给付费商品用户。

注意不要让赠品抵扣导致用户应付为负。如果某用户折扣超过商品金额，超出部分应转入全局 rounding_adjustment 或按政策分摊给其他用户。

二十三、结算主流程伪代码

```rust
pub fn settle_round(input: SettlementInput) -> Result<SettlementSnapshot, SettlementError> {
    let mut bills = build_user_bill_lines(&input.allocation, &input.items);

    let gross_total = bills.iter().map(|b| b.gross_total).sum_money()?;

    let mut discount_applications = Vec::new();

    for rule in &input.discount_rules {
        match rule {
            DiscountRule::ThresholdDiscount { threshold, discount, repeatable, scope, .. } => {
                let scoped_basis = compute_user_basis(&bills, scope);
                let scoped_total = sum_basis(&scoped_basis);
                let times = if *repeatable { scoped_total.0 / threshold.0 } else if scoped_total.0 >= threshold.0 { 1 } else { 0 };
                let amount = discount.checked_mul_i64(times)?;
                let shares = allocate_discount_by_ratio(amount, &scoped_basis);
                apply_discount_shares(&mut bills, &shares);
                discount_applications.push(DiscountApplication::from(rule, amount, shares));
            }
            DiscountRule::FixedActualDiscount { amount, scope, allocation_policy, .. } => {
                let basis = compute_user_basis(&bills, scope);
                let shares = allocate_by_policy(*amount, &basis, allocation_policy)?;
                apply_discount_shares(&mut bills, &shares);
                discount_applications.push(...);
            }
            DiscountRule::GiftByThreshold { threshold, gift_valuation, value_offset_policy, .. } => {
                let basis = compute_user_basis(&bills, &DiscountScope::AllPaidItems);
                let scoped_total = sum_basis(&basis);
                let gift_count = scoped_total.0 / threshold.0;
                let total_gift_value = gift_valuation.checked_mul_i64(gift_count)?;
                let shares = allocate_by_policy(total_gift_value, &basis, value_offset_policy)?;
                apply_gift_value_shares(&mut bills, &shares);
                discount_applications.push(...);
            }
            _ => {}
        }
    }

    for bill in &mut bills {
        bill.final_total = bill.gross_total
            .checked_sub(bill.discount_share)?
            .checked_sub(bill.gift_value_share)?
            .checked_add(bill.shipping_fee)?;
        if bill.final_total.0 < 0 {
            // 业务上不允许负数，截断并记录 warning
        }
    }

    Ok(SettlementSnapshot { ... })
}
```

二十四、管理员自然语言优惠解析

管理员输入自然语言后，也由 LLM 转 JSON，但仍要校验。

示例输入：

```text
设置优惠：满300-50，满800-120，购物金80，满500送特典1张，特典估30，按金额比例摊。
```

LLM 输出：

```json
{
  "intent": "AdminCommand",
  "admin_command": {
    "type": "SetDiscountRules",
    "rules": [
      {"type":"ThresholdDiscount", "threshold_cents":30000, "discount_cents":5000, "repeatable":false, "stackable":true, "scope":"AllPaidItems"},
      {"type":"ThresholdDiscount", "threshold_cents":80000, "discount_cents":12000, "repeatable":false, "stackable":true, "scope":"AllPaidItems"},
      {"type":"FixedActualDiscount", "amount_cents":8000, "scope":"AllPaidItems", "allocation_policy":"ByGrossAmountRatio"},
      {"type":"GiftByThreshold", "threshold_cents":50000, "gift_name":"特典", "gift_quantity_per_threshold":1, "gift_valuation_cents":3000, "allocation_policy":"TreatAsSplitItem", "value_offset_policy":"ByGrossAmountRatio"}
    ]
  },
  "confidence": 0.9,
  "ambiguous_parts": []
}
```

程序校验重点：

1. threshold 和 discount 必须大于 0。

2. gift_name 必须能匹配到 item_kind=Gift 的商品，否则要求管理员先创建赠品商品。

3. 不允许优惠金额超过作用范围总金额，除非管理员确认。

4. 规则顺序必须保存，因为叠加优惠时顺序可能影响展示。

二十五、前端快照格式

R2 中建议输出 current.json，前端只读这个文件。

```json
{
  "round_id": "es_2026_05",
  "title": "ES 5月新谷",
  "status": "active",
  "version": 128,
  "updated_at": "2026-05-08T21:12:00Z",
  "items": [
    {
      "item_id": "badge_rinne",
      "name": "燐音吧唧",
      "kind": "split",
      "unit_price_cents": 4500,
      "boxes": [
        {
          "box_index": 1,
          "slots": [
            {"slot_index":1, "status":"filled", "user_id":"456", "display_name":"A", "policy":"normal"},
            {"slot_index":2, "status":"empty", "policy":"normal"}
          ]
        }
      ],
      "waiting": []
    }
  ],
  "user_bills": [
    {
      "user_id": "456",
      "display_name": "A",
      "gross_total_cents": 9000,
      "discount_share_cents": 800,
      "gift_value_share_cents": 300,
      "shipping_fee_cents": 0,
      "final_total_cents": 7900
    }
  ],
  "warnings": []
}
```

R2 路径：

```text
rounds/{round_id}/current.json
rounds/{round_id}/snapshots/{version}.json
rounds/{round_id}/exports/summary.csv
rounds/{round_id}/exports/user_bills.csv
```

二十六、R2 Publisher 接口

```rust
#[async_trait::async_trait]
pub trait SnapshotPublisher: Send + Sync {
    async fn publish_current(&self, round_id: &RoundId, snapshot: &PublicSnapshot) -> Result<(), PublishError>;
    async fn publish_versioned(&self, round_id: &RoundId, version: i64, snapshot: &PublicSnapshot) -> Result<(), PublishError>;
}
```

R2 实现只负责 put_object。序列化在 SnapshotService 中完成。

二十七、导出功能

结团时至少输出三类表。

1. 商品汇总表 item_summary.csv

字段：item_id、商品名、类型、单价、数量、原价合计、已成盒数、未成盒数、赠品数量、备注。

2. 用户账单表 user_bills.csv

字段：用户ID、昵称、商品明细、拼团原价、单领原价、优惠分摊、赠品估值抵扣、邮费、最终应付、付款状态。

3. 下单辅助表 order_helper.csv

字段：商品名、购买数量、平台下单单价、总额、参与优惠范围、备注。

Rust CSV 伪代码：

```rust
pub fn export_user_bills(snapshot: &SettlementSnapshot) -> Result<Vec<u8>, ExportError> {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record(["用户ID", "昵称", "商品明细", "原价", "优惠", "赠品抵扣", "邮费", "最终应付"])?;

    for bill in &snapshot.user_bills {
        let detail = bill.lines.iter()
            .map(|l| format!("{} x{}", l.item_name, l.quantity))
            .collect::<Vec<_>>()
            .join("；");

        wtr.write_record([
            bill.user_id.0.as_str(),
            bill.display_name.as_str(),
            detail.as_str(),
            &format_money(bill.gross_total),
            &format_money(bill.discount_share),
            &format_money(bill.gift_value_share),
            &format_money(bill.shipping_fee),
            &format_money(bill.final_total),
        ])?;
    }

    Ok(wtr.into_inner()?)
}
```

二十八、管理员命令设计

虽然管理员可以用自然语言，但建议同时支持结构化命令，便于排错。

推荐命令：

```text
/开团 标题=ES5月新谷 开始=2026-05-08 20:00
/加商品 名称=燐音吧唧 类型=拼团 单价=45 盒规=10 别名=燐音,rinne,天城燐音
/加商品 名称=蓝良立牌 类型=单领 单价=60 库存=5 别名=蓝良,aira
/加优先 用户=@A 等级=10 范围=全部 备注=购物金
/锁位 商品=燐音吧唧 盒=2 位=5
/修正 商品=燐音吧唧 用户=@A 盒=1 位=3
/设置优惠 满300-50，购物金80，满500送特典1张估30按金额比例摊
/结团
/导出
```

自然语言命令进入 LLM；斜杠命令可以用程序直接解析，失败再交给 LLM。

二十九、错误与回复策略

BotReply 类型：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BotReply {
    Silent,
    Text(String),
    NeedConfirm { text: String, confirm_token: String },
    AdminOnly(String),
}
```

常见回复：

1. 成功记录：

```text
已记录：燐音吧唧 x2，蓝良立牌单领 x1。当前版本 #128。
```

2. 歧义：

```text
“燐音”匹配到多个商品：燐音吧唧、燐音色纸。请回复：确认 燐音吧唧 2。
```

3. 不在开团时间：

```text
当前团尚未开始，普通排谷不记录。若你有优先权，请确认管理员已录入。
```

4. 撤销成功：

```text
已撤销你最近一条排谷，当前版本 #129。
```

5. 解析失败：

```text
没有识别到明确商品和数量，请按“排 商品名 数量”的格式重发。
```

三十、并发与一致性

因为手速严格依赖时间戳和消息顺序，必须避免并发处理同一个 round 导致乱序。

建议策略：

1. raw_messages 可并发入库。

2. 对同一个 round 的事件写入和重放必须串行。

3. 使用数据库事务 + advisory lock，锁 key = hash(round_id)。

PostgreSQL advisory lock 示例：

```rust
async fn with_round_lock<T, F, Fut>(&self, round_id: &RoundId, f: F) -> Result<T, AppError>
where
    F: FnOnce(&mut Transaction<'_, Postgres>) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let mut tx = self.pool.begin().await?;
    let lock_key = hash_round_id_to_i64(round_id);
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;

    let result = f(&mut tx).await?;
    tx.commit().await?;
    Ok(result)
}
```

如果你的消息网关已经保证单群串行，也仍建议保留数据库锁，防止管理员 API 和群消息同时修改。

三十一、测试用例必须覆盖

1. 普通用户按时间戳排入同一商品。

2. 有优先权用户晚发，但排在无优先权用户前。

3. 优先权只对指定商品生效。

4. 用户混合发送拼团和单领，能拆成多条 claim line。

5. 商品名歧义时不落事件。

6. 撤销最近一条后，后面的普通用户自动前移。

7. 包尾用户创建独立锁定盒，不填补前面空位。

8. 包尾用户不阻塞后续普通用户填补前面空位。

9. 管理员锁位后，普通用户不能占该槽。

10. 满减按金额比例分摊，分摊后总分等于优惠总分。

11. 赠品估值抵扣后总账平衡。

12. 多 round 同时 active 时，商品唯一匹配则自动进入正确 round；不唯一则要求确认。

测试示例：

```rust
#[test]
fn priority_user_should_be_allocated_before_normal_user() {
    let item = test_split_item("badge", 10);
    let normal = claim_line("u1", "badge", 1, ts(100), 1, 0);
    let priority = claim_line("u2", "badge", 1, ts(101), 2, 10);

    let snapshot = allocate(vec![item], vec![normal, priority]);
    let slots = snapshot.item("badge").box_at(1).slots;

    assert_eq!(slots[0].user_id.as_deref(), Some("u2"));
    assert_eq!(slots[1].user_id.as_deref(), Some("u1"));
}

#[test]
fn tail_locked_should_not_fill_previous_empty_slots() {
    let item = test_split_item("bonus", 5);
    let a = normal_line("u1", "bonus", 1, ts(100));
    let tail = tail_line("u2", "bonus", 2, ts(101));
    let b = normal_line("u3", "bonus", 1, ts(102));

    let snapshot = allocate(vec![item], vec![a, tail, b]);
    let item_alloc = snapshot.item("bonus");

    assert_eq!(item_alloc.box_at(1).slot(1).user_id(), Some("u1"));
    assert_eq!(item_alloc.box_at(1).slot(2).user_id(), Some("u3"));
    assert_eq!(item_alloc.box_at(2).slot(1).user_id(), Some("u2"));
    assert_eq!(item_alloc.box_at(2).slot(2).user_id(), Some("u2"));
    assert!(item_alloc.box_at(2).slot(3).is_locked_empty());
}
```

三十二、关键业务边界

1. LLM 输出不能直接写 events，必须经过 validation。

2. 所有金额计算只用 cents 整数。

3. round status 不是 active 时，普通用户 claim 默认不记录，管理员命令除外。

4. 结团 closed 后，不允许普通 claim/cancel，除非管理员 reopen 或 admin adjustment。

5. 每条 raw message 必须幂等。相同 group_id + qq_message_id 不重复处理。

6. 每个 snapshot 必须有 version，version 可用事件最大 sequence。

7. 发布 R2 失败不能回滚业务事件，但必须记录 publish_failed 状态并允许后台或管理员重试。

8. 导出使用最新 snapshot，不临时重新拼复杂表，避免导出和展示不一致。

三十三、推荐 Cargo.toml 依赖

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
thiserror = "1"
anyhow = "1"
async-trait = "0.1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "chrono", "uuid", "json"] }
csv = "1"
reqwest = { version = "0.12", features = ["json", "stream"] }
aws-config = "1"
aws-sdk-s3 = "1"
```

三十四、main.rs 入口示例

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;

    let app_state = AppState::build(config, pool).await?;
    let app = api::routes::build_router(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

三十五、API 路由建议

虽然消息收发已有框架，但后端仍建议暴露 HTTP API，便于管理后台和测试。

```text
WS  ws://0.0.0.0:3001              接收 QQ 框架消息（反向WS服务器）
POST /webhook/qq-message          接收已解析的 QQ 框架消息（HTTP 备选）
POST /admin/rounds                创建团
POST /admin/rounds/{id}/items      添加商品
POST /admin/rounds/{id}/discounts  设置优惠
POST /admin/rounds/{id}/close      结团
GET  /public/rounds/{id}/current   读取当前快照
GET  /admin/rounds/{id}/export     导出
GET  /api/replays/{id}/replays     列出该团所有 replay
GET  /api/simulations              提交模拟任务
```

三十六、最终给 LLM 生成代码时的实现顺序建议

虽然你说不需要分阶段，但一次性生成时仍要按依赖顺序实现，否则代码模型容易乱。建议在提示词里要求它按以下顺序写：

```text
1. domain 类型
2. error 类型
3. repository trait
4. parser trait 和 mock parser
5. validation
6. event replay
7. allocation engine
8. settlement engine
9. snapshot service
10. message service
11. axum routes
12. postgres repo skeleton
13. tests
```

三十七、最重要的实现约束总结

系统只有一个核心事实源：events 表。raw_messages 是证据，parsed_messages 是解释，snapshots 是缓存，exports 是产物。不要让 snapshots 或 exports 反向成为业务事实源。

LLM 的角色是翻译人话，不是做账房，也不是做裁判。凡是涉及排位、优先权、撤销后前移、包尾锁列、满减分摊、赠品估值抵扣，都必须由 Rust 代码中的确定性函数处理。

包尾和端盒必须建模为 SlotPolicy + Segment，而不是备注。否则它一定会被普通补位逻辑吞掉。

优惠和赠品必须和排队拆开。排队回答“谁拿什么”，结算回答“谁付多少”。赠品同时参与物理分配和财务抵扣，所以 Gift Item 与 Gift Valuation 必须分离。

只要坚持事件流、确定性重放、LLM 只解析这三点，这个系统虽然业务复杂，但代码结构不会失控。
