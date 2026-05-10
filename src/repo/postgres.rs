use sqlx::{PgPool, Postgres, Row, Transaction};
use async_trait::async_trait;
use serde_json::Value as JsonValue;

use crate::domain::ids::{AliasId, EligibilityId, EventId, ItemId, RoundId, UserId};
use crate::domain::round::{Round, RoundStatus};
use crate::domain::item::{Item, ItemAlias, ItemKind};
use crate::domain::claim::{Eligibility, EligibilityScope};
use crate::domain::event::{DomainEvent, EventEnvelope, EventStatus};
use crate::domain::snapshot::AllocationSnapshot;
use crate::domain::settlement::SettlementSnapshot;
use crate::domain::user::User;
use crate::domain::money::MoneyCents;
use crate::error::{AppError, AppResult, ParseError};
use crate::repo::round_repo::{
    RoundRepo, ItemRepo, EventRepo, SnapshotRepo, RawMessageRepo, RawMessageRecord, EligibilityRepo,
};
use crate::repo::user_repo::UserRepo;

fn event_status_to_str(s: &EventStatus) -> &'static str {
    match s {
        EventStatus::Active => "active",
        EventStatus::Superseded => "superseded",
    }
}

fn event_status_from_str(s: &str) -> Option<EventStatus> {
    match s {
        "active" => Some(EventStatus::Active),
        "superseded" => Some(EventStatus::Superseded),
        _ => None,
    }
}

// ---- Struct definitions ----

#[derive(Clone)]
pub struct PgRoundRepo { pub(crate) pool: PgPool }

#[derive(Clone)]
pub struct PgItemRepo { pub(crate) pool: PgPool }

#[derive(Clone)]
pub struct PgEventRepo { pub(crate) pool: PgPool }

#[derive(Clone)]
pub struct PgSnapshotRepo { pub(crate) pool: PgPool }

#[derive(Clone)]
pub struct PgRawMessageRepo { pub(crate) pool: PgPool }

#[derive(Clone)]
pub struct PgEligibilityRepo { pub(crate) pool: PgPool }

#[derive(Clone)]
pub struct PgUserRepo { pub(crate) pool: PgPool }

impl PgRoundRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }
impl PgItemRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }
impl PgEventRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }
impl PgSnapshotRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }
impl PgRawMessageRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }
impl PgEligibilityRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }
impl PgUserRepo { pub fn new(pool: PgPool) -> Self { Self { pool } } }

// ---- Row mapping helpers ----

fn row_to_round(row: &sqlx::postgres::PgRow) -> AppResult<Round> {
    let status_str: String = row.try_get("status")?;
    Ok(Round {
        round_id: RoundId(row.try_get("round_id")?),
        group_id: row.try_get("group_id")?,
        title: row.try_get("title")?,
        status: RoundStatus::from_str(&status_str).unwrap_or(RoundStatus::Draft),
        start_at: row.try_get("start_at")?,
        end_at: row.try_get("end_at")?,
        allow_cancel: row.try_get("allow_cancel")?,
        allow_modify: row.try_get("allow_modify")?,
        default_timezone: row.try_get("default_timezone")?,
        created_by: row.try_get("created_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_item(row: &sqlx::postgres::PgRow) -> AppResult<Item> {
    let kind_str: String = row.try_get("kind")?;
    let aliases_json: JsonValue = row.try_get("aliases")?;
    let aliases: Vec<String> = serde_json::from_value(aliases_json).unwrap_or_default();
    Ok(Item {
        item_id: ItemId(row.try_get("item_id")?),
        round_id: RoundId(row.try_get("round_id")?),
        name: row.try_get("name")?,
        kind: ItemKind::from_str(&kind_str).unwrap_or(ItemKind::Split),
        unit_price: MoneyCents(row.try_get::<i64, _>("unit_price")?),
        box_size: row.try_get::<Option<i32>, _>("box_size")?.map(|v| v as u32),
        max_quantity: row.try_get::<Option<i32>, _>("max_quantity")?.map(|v| v as u32),
        is_blind: row.try_get("is_blind")?,
        is_proxy_card: row.try_get("is_proxy_card")?,
        aliases,
        sort_order: row.try_get("sort_order")?,
        metadata: row.try_get("metadata")?,
    })
}

fn row_to_item_alias(row: &sqlx::postgres::PgRow) -> AppResult<ItemAlias> {
    Ok(ItemAlias {
        alias_id: AliasId(row.try_get("alias_id")?),
        round_id: RoundId(row.try_get("round_id")?),
        item_id: ItemId(row.try_get("item_id")?),
        alias: row.try_get("alias")?,
        weight: row.try_get("weight")?,
    })
}

fn row_to_event_envelope(row: &sqlx::postgres::PgRow) -> AppResult<EventEnvelope> {
    let payload_json: JsonValue = row.try_get("payload")?;
    let payload: DomainEvent = serde_json::from_value(payload_json)
        .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
    let status_str: String = row.try_get("status")?;
    Ok(EventEnvelope {
        event_id: EventId(row.try_get("event_id")?),
        round_id: RoundId(row.try_get("round_id")?),
        group_id: row.try_get("group_id")?,
        user_id: UserId(row.try_get("user_id")?),
        raw_message_id: row.try_get("raw_message_id")?,
        event_type: row.try_get("event_type")?,
        effective_at: row.try_get("effective_at")?,
        sequence: row.try_get("sequence")?,
        payload,
        status: event_status_from_str(&status_str).unwrap_or(EventStatus::Active),
    })
}

fn row_to_eligibility(row: &sqlx::postgres::PgRow) -> AppResult<Eligibility> {
    let scope_json: JsonValue = row.try_get("scope")?;
    let scope: EligibilityScope = serde_json::from_value(scope_json)
        .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
    Ok(Eligibility {
        eligibility_id: EligibilityId(row.try_get("eligibility_id")?),
        round_id: RoundId(row.try_get("round_id")?),
        user_id: UserId(row.try_get("user_id")?),
        priority_type: row.try_get("priority_type")?,
        priority_level: row.try_get("priority_level")?,
        scope,
        max_uses: row.try_get("max_uses")?,
        used_count: row.try_get("used_count")?,
        valid_from: row.try_get("valid_from")?,
        valid_until: row.try_get("valid_until")?,
        note: row.try_get("note")?,
    })
}

fn row_to_raw_message(row: &sqlx::postgres::PgRow) -> AppResult<RawMessageRecord> {
    Ok(RawMessageRecord {
        raw_message_id: row.try_get("raw_message_id")?,
        group_id: row.try_get("group_id")?,
        user_id: row.try_get("user_id")?,
        qq_message_id: row.try_get("qq_message_id")?,
        timestamp: row.try_get("timestamp")?,
        text: row.try_get("text")?,
        images: row.try_get("images")?,
        is_admin: row.try_get("is_admin")?,
    })
}

fn row_to_user(row: &sqlx::postgres::PgRow) -> AppResult<User> {
    Ok(User {
        user_id: UserId(row.try_get("user_id")?),
        qq_id: row.try_get("qq_id")?,
        display_name: row.try_get("display_name")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

// ---- PgRoundRepo ----

#[async_trait]
impl RoundRepo for PgRoundRepo {
    async fn find_by_id(&self, round_id: &RoundId) -> AppResult<Option<Round>> {
        let row = sqlx::query(
            "SELECT round_id, group_id, title, status, start_at, end_at, allow_cancel, allow_modify, default_timezone, created_by, created_at, updated_at FROM rounds WHERE round_id = $1"
        )
        .bind(&round_id.0)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(Some(row_to_round(&row)?)),
            None => Ok(None),
        }
    }

    async fn find_active_by_group(&self, group_id: &str) -> AppResult<Vec<Round>> {
        let rows = sqlx::query(
            "SELECT round_id, group_id, title, status, start_at, end_at, allow_cancel, allow_modify, default_timezone, created_by, created_at, updated_at FROM rounds WHERE group_id = $1 AND status = 'active'"
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_round(row)).collect()
    }

    async fn find_all_by_group(&self, group_id: &str) -> AppResult<Vec<Round>> {
        let rows = sqlx::query(
            "SELECT round_id, group_id, title, status, start_at, end_at, allow_cancel, allow_modify, default_timezone, created_by, created_at, updated_at FROM rounds WHERE group_id = $1 ORDER BY created_at DESC"
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_round(row)).collect()
    }

    async fn insert(&self, round: &Round) -> AppResult<Round> {
        sqlx::query(
            "INSERT INTO rounds (round_id, group_id, title, status, start_at, end_at, allow_cancel, allow_modify, default_timezone, created_by, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(&round.round_id.0)
        .bind(&round.group_id)
        .bind(&round.title)
        .bind(round.status.as_str())
        .bind(round.start_at)
        .bind(round.end_at)
        .bind(round.allow_cancel)
        .bind(round.allow_modify)
        .bind(&round.default_timezone)
        .bind(&round.created_by)
        .bind(round.created_at)
        .bind(round.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(round.clone())
    }

    async fn update_status(&self, round_id: &RoundId, status: RoundStatus) -> AppResult<()> {
        sqlx::query(
            "UPDATE rounds SET status = $1, updated_at = NOW() WHERE round_id = $2"
        )
        .bind(status.as_str())
        .bind(&round_id.0)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ---- PgItemRepo ----

#[async_trait]
impl ItemRepo for PgItemRepo {
    async fn find_by_round(&self, round_id: &RoundId) -> AppResult<Vec<Item>> {
        let rows = sqlx::query(
            "SELECT item_id, round_id, name, kind, unit_price, box_size, max_quantity, is_blind, is_proxy_card, aliases, sort_order, metadata FROM items WHERE round_id = $1 ORDER BY sort_order"
        )
        .bind(&round_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_item(row)).collect()
    }

    async fn find_by_id(&self, item_id: &ItemId) -> AppResult<Option<Item>> {
        let row = sqlx::query(
            "SELECT item_id, round_id, name, kind, unit_price, box_size, max_quantity, is_blind, is_proxy_card, aliases, sort_order, metadata FROM items WHERE item_id = $1"
        )
        .bind(&item_id.0)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(Some(row_to_item(&row)?)),
            None => Ok(None),
        }
    }

    async fn insert(&self, item: &Item) -> AppResult<Item> {
        let aliases_json = serde_json::to_value(&item.aliases)
            .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
        sqlx::query(
            "INSERT INTO items (item_id, round_id, name, kind, unit_price, box_size, max_quantity, is_blind, is_proxy_card, aliases, sort_order, metadata) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(&item.item_id.0)
        .bind(&item.round_id.0)
        .bind(&item.name)
        .bind(item.kind.as_str())
        .bind(item.unit_price.0)
        .bind(item.box_size.map(|v| v as i32))
        .bind(item.max_quantity.map(|v| v as i32))
        .bind(item.is_blind)
        .bind(item.is_proxy_card)
        .bind(&aliases_json)
        .bind(item.sort_order)
        .bind(&item.metadata)
        .execute(&self.pool)
        .await?;
        Ok(item.clone())
    }

    async fn find_aliases_by_round(&self, round_id: &RoundId) -> AppResult<Vec<ItemAlias>> {
        let rows = sqlx::query(
            "SELECT alias_id, round_id, item_id, alias, weight FROM item_aliases WHERE round_id = $1"
        )
        .bind(&round_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_item_alias(row)).collect()
    }
}

// ---- PgEventRepo ----

#[async_trait]
impl EventRepo for PgEventRepo {
    async fn insert(&self, event: &EventEnvelope) -> AppResult<EventEnvelope> {
        let payload_json = serde_json::to_value(&event.payload)
            .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
        sqlx::query(
            "INSERT INTO events (event_id, round_id, group_id, user_id, raw_message_id, event_type, effective_at, sequence, payload, status) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        )
        .bind(&event.event_id.0)
        .bind(&event.round_id.0)
        .bind(&event.group_id)
        .bind(&event.user_id.0)
        .bind(&event.raw_message_id)
        .bind(&event.event_type)
        .bind(event.effective_at)
        .bind(event.sequence)
        .bind(&payload_json)
        .bind(event_status_to_str(&event.status))
        .execute(&self.pool)
        .await?;
        Ok(event.clone())
    }

    async fn find_by_round(&self, round_id: &RoundId) -> AppResult<Vec<EventEnvelope>> {
        let rows = sqlx::query(
            "SELECT event_id, round_id, group_id, user_id, raw_message_id, event_type, effective_at, sequence, payload, status FROM events WHERE round_id = $1 ORDER BY sequence ASC"
        )
        .bind(&round_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_event_envelope(row)).collect()
    }

    async fn find_by_id(&self, event_id: &EventId) -> AppResult<Option<EventEnvelope>> {
        let row = sqlx::query(
            "SELECT event_id, round_id, group_id, user_id, raw_message_id, event_type, effective_at, sequence, payload, status FROM events WHERE event_id = $1"
        )
        .bind(&event_id.0)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(Some(row_to_event_envelope(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_max_sequence(&self, round_id: &RoundId) -> AppResult<i64> {
        let row = sqlx::query(
            "SELECT COALESCE(MAX(sequence), 0) as max_seq FROM events WHERE round_id = $1"
        )
        .bind(&round_id.0)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get("max_seq")?)
    }
}

// ---- PgSnapshotRepo ----

#[async_trait]
impl SnapshotRepo for PgSnapshotRepo {
    async fn save_allocation(&self, snapshot: &AllocationSnapshot) -> AppResult<()> {
        let snapshot_json = serde_json::to_value(snapshot)
            .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
        sqlx::query(
            "INSERT INTO allocation_snapshots (round_id, version, generated_at, snapshot_json) VALUES ($1, $2, $3, $4)"
        )
        .bind(&snapshot.round_id.0)
        .bind(snapshot.version)
        .bind(snapshot.generated_at)
        .bind(&snapshot_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn save_settlement(&self, snapshot: &SettlementSnapshot) -> AppResult<()> {
        let snapshot_json = serde_json::to_value(snapshot)
            .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
        sqlx::query(
            "INSERT INTO settlement_snapshots (round_id, version, generated_at, snapshot_json) VALUES ($1, $2, $3, $4)"
        )
        .bind(&snapshot.round_id.0)
        .bind(snapshot.version)
        .bind(snapshot.generated_at)
        .bind(&snapshot_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_latest_allocation(&self, round_id: &RoundId) -> AppResult<Option<AllocationSnapshot>> {
        let row = sqlx::query(
            "SELECT snapshot_json FROM allocation_snapshots WHERE round_id = $1 ORDER BY version DESC LIMIT 1"
        )
        .bind(&round_id.0)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => {
                let json: JsonValue = row.try_get("snapshot_json")?;
                let snapshot = serde_json::from_value(json)
                    .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }

    async fn get_latest_settlement(&self, round_id: &RoundId) -> AppResult<Option<SettlementSnapshot>> {
        let row = sqlx::query(
            "SELECT snapshot_json FROM settlement_snapshots WHERE round_id = $1 ORDER BY version DESC LIMIT 1"
        )
        .bind(&round_id.0)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => {
                let json: JsonValue = row.try_get("snapshot_json")?;
                let snapshot = serde_json::from_value(json)
                    .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }
}

// ---- PgRawMessageRepo ----

#[async_trait]
impl RawMessageRepo for PgRawMessageRepo {
    async fn insert_raw_message(&self, msg: &RawMessageRecord) -> AppResult<RawMessageRecord> {
        sqlx::query(
            "INSERT INTO raw_messages (raw_message_id, group_id, user_id, qq_message_id, timestamp, text, images, is_admin) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        )
        .bind(&msg.raw_message_id)
        .bind(&msg.group_id)
        .bind(&msg.user_id)
        .bind(&msg.qq_message_id)
        .bind(msg.timestamp)
        .bind(&msg.text)
        .bind(&msg.images)
        .bind(msg.is_admin)
        .execute(&self.pool)
        .await?;
        Ok(msg.clone())
    }

    async fn find_by_message_id(&self, group_id: &str, qq_message_id: &str) -> AppResult<Option<RawMessageRecord>> {
        let row = sqlx::query(
            "SELECT raw_message_id, group_id, user_id, qq_message_id, timestamp, text, images, is_admin FROM raw_messages WHERE group_id = $1 AND qq_message_id = $2"
        )
        .bind(group_id)
        .bind(qq_message_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(Some(row_to_raw_message(&row)?)),
            None => Ok(None),
        }
    }
}

// ---- PgEligibilityRepo ----

#[async_trait]
impl EligibilityRepo for PgEligibilityRepo {
    async fn find_by_round(&self, round_id: &RoundId) -> AppResult<Vec<Eligibility>> {
        let rows = sqlx::query(
            "SELECT eligibility_id, round_id, user_id, priority_type, priority_level, scope, max_uses, used_count, valid_from, valid_until, note FROM eligibilities WHERE round_id = $1"
        )
        .bind(&round_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_eligibility(row)).collect()
    }

    async fn find_by_user_and_round(&self, user_id: &UserId, round_id: &RoundId) -> AppResult<Vec<Eligibility>> {
        let rows = sqlx::query(
            "SELECT eligibility_id, round_id, user_id, priority_type, priority_level, scope, max_uses, used_count, valid_from, valid_until, note FROM eligibilities WHERE user_id = $1 AND round_id = $2"
        )
        .bind(&user_id.0)
        .bind(&round_id.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| row_to_eligibility(row)).collect()
    }

    async fn insert(&self, eligibility: &Eligibility) -> AppResult<Eligibility> {
        let scope_json = serde_json::to_value(&eligibility.scope)
            .map_err(|e| AppError::Parse(ParseError::InvalidJson(e)))?;
        sqlx::query(
            "INSERT INTO eligibilities (eligibility_id, round_id, user_id, priority_type, priority_level, scope, max_uses, used_count, valid_from, valid_until, note) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
        )
        .bind(&eligibility.eligibility_id.0)
        .bind(&eligibility.round_id.0)
        .bind(&eligibility.user_id.0)
        .bind(&eligibility.priority_type)
        .bind(eligibility.priority_level)
        .bind(&scope_json)
        .bind(eligibility.max_uses)
        .bind(eligibility.used_count)
        .bind(eligibility.valid_from)
        .bind(eligibility.valid_until)
        .bind(&eligibility.note)
        .execute(&self.pool)
        .await?;
        Ok(eligibility.clone())
    }
}

// ---- PgUserRepo ----

#[async_trait]
impl UserRepo for PgUserRepo {
    async fn find_by_id(&self, user_id: &UserId) -> AppResult<Option<User>> {
        let row = sqlx::query(
            "SELECT user_id, qq_id, display_name, created_at, updated_at FROM users WHERE user_id = $1"
        )
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    async fn find_by_qq_id(&self, qq_id: &str) -> AppResult<Option<User>> {
        let row = sqlx::query(
            "SELECT user_id, qq_id, display_name, created_at, updated_at FROM users WHERE qq_id = $1"
        )
        .bind(qq_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    async fn upsert(&self, user: &User) -> AppResult<User> {
        let row = sqlx::query(
            "INSERT INTO users (user_id, qq_id, display_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (user_id) DO UPDATE SET qq_id = $2, display_name = $3, updated_at = $5 RETURNING user_id, qq_id, display_name, created_at, updated_at"
        )
        .bind(&user.user_id.0)
        .bind(&user.qq_id)
        .bind(&user.display_name)
        .bind(user.created_at)
        .bind(user.updated_at)
        .fetch_one(&self.pool)
        .await?;
        row_to_user(&row)
    }
}

// ---- Advisory Lock Helper ----

pub(crate) async fn with_round_lock<F, Fut, T>(
    pool: &PgPool,
    round_id: &RoundId,
    f: F,
) -> AppResult<T>
where
    F: FnOnce(&mut Transaction<'_, Postgres>) -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
{
    let key: i64 = round_id.0.as_bytes().iter().fold(0i64, |acc, &b| acc + b as i64);
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    let result = f(&mut tx).await?;
    tx.commit().await?;
    Ok(result)
}
