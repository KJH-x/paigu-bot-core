use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::claim::{ClaimLine, ClaimType, SlotPolicy};
use crate::domain::event::{ClaimCancelled, ClaimCreated, DomainEvent, EventEnvelope, EventStatus};
use crate::domain::ids::{ClaimId, EventId, ItemId, RoundId, UserId};
use crate::messages::processing::{ProcessingOutcome, Understanding, UnderstandingSource};
use crate::messages::MessageRecord;
use crate::settings::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionSelection {
    Automatic {
        note: String,
    },
    Replace {
        actions: Vec<EditableAction>,
        note: String,
    },
    Ignore {
        note: String,
    },
    Reject {
        note: String,
    },
}

impl ActionSelection {
    pub fn note(&self) -> &str {
        match self {
            Self::Automatic { note }
            | Self::Replace { note, .. }
            | Self::Ignore { note }
            | Self::Reject { note } => note,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Claim,
    Cancel,
    Modify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditableAction {
    pub action_id: String,
    pub operation: Operation,
    #[serde(default)]
    pub items: Vec<EditableItem>,
    #[serde(default)]
    pub target_claim_id: Option<String>,
    #[serde(default)]
    pub target_item_id: Option<String>,
    #[serde(default)]
    pub quantity: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditableItem {
    pub line_id: String,
    pub item_id: String,
    pub subitem_id: Option<String>,
    pub quantity: u32,
    pub claim_type: ClaimType,
    pub slot_policy: SlotPolicy,
    pub is_proxy_card: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSelection {
    pub action_selection_version: u8,
    pub transaction_id: String,
    pub request_hash: String,
    pub selection: ActionSelection,
    pub events: Vec<EventEnvelope>,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn primary_claim_id(record: &MessageRecord) -> String {
    record
        .processing
        .as_ref()
        .and_then(|p| p.results.first())
        .and_then(|r| {
            r.understanding
                .events
                .iter()
                .find_map(|e| match &e.payload {
                    DomainEvent::ClaimCreated(c) => Some(c.claim_id.0.clone()),
                    _ => None,
                })
        })
        .unwrap_or_else(|| {
            if record.processing.is_some() {
                format!("claim-{}", record.record_id)
            } else {
                format!("claim-{}", record.seq)
            }
        })
}

pub fn compile(
    cfg: &AppConfig,
    rec: &MessageRecord,
    selection: &ActionSelection,
) -> Result<Understanding, String> {
    if selection.note().chars().count() > 2000 {
        return Err("管理员解释最多 2000 字".into());
    }
    let mut result =
        Understanding::terminal(UnderstandingSource::Control, "Ignored", "人工忽略", None);
    let actions = match selection {
        ActionSelection::Automatic { .. } => return Err("恢复自动须使用已存自动结果".into()),
        ActionSelection::Ignore { .. } => return Ok(result),
        ActionSelection::Reject { .. } => {
            result.validation = ProcessingOutcome::new("Rejected", "人工拒绝", None);
            return Ok(result);
        }
        ActionSelection::Replace { actions, .. } => actions,
    };
    if actions.len() > 32 {
        return Err("单条消息最多 32 个动作".into());
    }
    let now = DateTime::<Utc>::from_timestamp_millis(rec.timestamp_ms).ok_or("原消息时间无效")?;
    let mut ids = HashSet::new();
    for action in actions {
        if !valid_id(&action.action_id) || !ids.insert(action.action_id.clone()) {
            return Err("动作标识重复或无效".into());
        }
        let envelope = |suffix: &str, payload: DomainEvent| EventEnvelope {
            event_id: EventId(format!(
                "evt-{}-{}{suffix}",
                rec.record_id, action.action_id
            )),
            round_id: RoundId(cfg.round.round_id.clone()),
            group_id: rec.group_id.clone(),
            user_id: UserId(rec.user_id.clone()),
            raw_message_id: Some(rec.message_id.clone()),
            event_type: payload.event_type_str().into(),
            effective_at: now,
            sequence: rec.seq,
            payload,
            status: EventStatus::Active,
        };
        if action.operation == Operation::Cancel {
            if !action.items.is_empty()
                || (action.target_claim_id.is_some()
                    && (action.target_item_id.is_some() || action.quantity.is_some()))
                || (action.target_item_id.is_none() && action.quantity.is_some())
            {
                return Err("撤销目标须为具体申购、商品或最近一单，不能混用".into());
            }
            if action.quantity.is_some_and(|n| n == 0 || n > 99) {
                return Err("撤销数量应为 1–99，留空表示全部".into());
            }
            if action
                .target_item_id
                .as_ref()
                .is_some_and(|id| !cfg.round.items.iter().any(|i| &i.item_id == id))
            {
                return Err("撤销商品已不存在".into());
            }
            result.events.push(envelope(
                "",
                DomainEvent::ClaimCancelled(ClaimCancelled {
                    target_claim_id: action.target_claim_id.clone().map(ClaimId),
                    target_item_id: action.target_item_id.clone().map(ItemId),
                    quantity: action.quantity,
                    reason: Some("人工修订".into()),
                    parse_trace: None,
                    validation_trace: vec![],
                }),
            ));
            continue;
        }
        if action.target_claim_id.is_some()
            || action.target_item_id.is_some()
            || action.quantity.is_some()
        {
            return Err("排/改不能携带撤销目标".into());
        }
        if action.items.is_empty() || action.items.len() > 64 {
            return Err("排/改须包含 1–64 个商品项".into());
        }
        let mut line_ids = HashSet::new();
        let mut lines = vec![];
        for line in &action.items {
            if line
                .notes
                .as_ref()
                .is_some_and(|n| n.chars().count() > 1000)
            {
                return Err("商品备注最多 1000 字".into());
            }
            if !valid_id(&line.line_id) || !line_ids.insert(&line.line_id) {
                return Err("商品项标识重复或无效".into());
            }
            let item = cfg
                .round
                .to_items()
                .into_iter()
                .find(|i| i.item_id.0 == line.item_id)
                .ok_or("商品已不存在，请重新选择")?;
            if line
                .subitem_id
                .as_ref()
                .is_some_and(|id| !item.subitems.iter().any(|s| &s.subitem_id == id))
            {
                return Err("子项不属于所选商品".into());
            }
            if line.quantity == 0 || line.quantity > 99 {
                return Err("数量应为 1–99 的整数".into());
            }
            if !item.kind.compatible_with(&line.claim_type) {
                return Err("认购类型与商品种类不符".into());
            }
            if matches!(line.slot_policy, SlotPolicy::AdminFixed) {
                return Err("动作编辑不能设置管理员固定位".into());
            }
            lines.push(ClaimLine {
                item_id: ItemId(line.item_id.clone()),
                subitem_id: line.subitem_id.clone(),
                quantity: line.quantity,
                claim_type: line.claim_type.clone(),
                slot_policy: line.slot_policy.clone(),
                is_proxy_card: line.is_proxy_card,
                notes: line.notes.clone(),
            });
        }
        if action.operation == Operation::Modify {
            let mut targets = HashSet::new();
            for line in &lines {
                if targets.insert(line.item_id.0.clone()) {
                    result.events.push(envelope(
                        &format!("-cancel-{}", line.item_id.0),
                        DomainEvent::ClaimCancelled(ClaimCancelled {
                            target_claim_id: None,
                            target_item_id: Some(line.item_id.clone()),
                            quantity: None,
                            reason: Some("改单".into()),
                            parse_trace: None,
                            validation_trace: vec![],
                        }),
                    ));
                }
            }
        }
        let claim_id = if action.action_id == "primary" {
            primary_claim_id(rec)
        } else {
            format!("claim-{}-{}", rec.record_id, action.action_id)
        };
        result.events.push(envelope(
            "",
            DomainEvent::ClaimCreated(ClaimCreated {
                claim_id: ClaimId(claim_id),
                user_id: UserId(rec.user_id.clone()),
                items: lines,
                source_text: rec.text.clone(),
                parse_trace: None,
                validation_trace: vec![],
            }),
        ));
    }
    result.validation = if result.events.is_empty() {
        ProcessingOutcome::new("Ignored", "人工修订：无动作", None)
    } else {
        ProcessingOutcome::new(
            "Applied",
            result
                .events
                .iter()
                .map(|e| describe_event(e, cfg))
                .collect::<Vec<_>>()
                .join("；"),
            None,
        )
    };
    Ok(result)
}

pub fn describe_event(event: &EventEnvelope, cfg: &AppConfig) -> String {
    let name = |id: &str, sub: Option<&str>| {
        let Some(item) = cfg.round.items.iter().find(|i| i.item_id == id) else {
            return "已移除商品".to_string();
        };
        let sub = sub.and_then(|v| item.subitems.iter().find(|s| s.subitem_id == v));
        format!(
            "{}{}",
            item.name,
            sub.map_or_else(String::new, |s| format!("·{}", s.name))
        )
    };
    match &event.payload {
        DomainEvent::ClaimCreated(c) => format!(
            "排 {}",
            c.items
                .iter()
                .map(|i| format!(
                    "{} × {}",
                    name(&i.item_id.0, i.subitem_id.as_deref()),
                    i.quantity
                ))
                .collect::<Vec<_>>()
                .join("、")
        ),
        DomainEvent::ClaimCancelled(c) => {
            if c.target_claim_id.is_some() {
                "撤销指定申购".into()
            } else if let Some(item) = &c.target_item_id {
                format!(
                    "撤 {} · {}",
                    name(&item.0, None),
                    c.quantity
                        .map_or_else(|| "全部".into(), |n| format!("{n} 份"))
                )
            } else {
                "撤销最近一笔申购".into()
            }
        }
        _ => "其他操作".into(),
    }
}

pub fn editable(events: &[EventEnvelope], is_modify: bool) -> Vec<EditableAction> {
    let mut result = vec![];
    for event in events {
        if is_modify && matches!(event.payload, DomainEvent::ClaimCancelled(_)) {
            continue;
        }
        let mut action = EditableAction {
            action_id: if result.is_empty() {
                "primary".into()
            } else {
                format!("action-{}", result.len())
            },
            operation: Operation::Claim,
            items: vec![],
            target_claim_id: None,
            target_item_id: None,
            quantity: None,
        };
        match &event.payload {
            DomainEvent::ClaimCreated(c) => {
                if is_modify {
                    action.operation = Operation::Modify;
                }
                action.items = c
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, line)| EditableItem {
                        line_id: format!("line-{i}"),
                        item_id: line.item_id.0.clone(),
                        subitem_id: line.subitem_id.clone(),
                        quantity: line.quantity,
                        claim_type: line.claim_type.clone(),
                        slot_policy: line.slot_policy.clone(),
                        is_proxy_card: line.is_proxy_card,
                        notes: line.notes.clone(),
                    })
                    .collect();
            }
            DomainEvent::ClaimCancelled(c) => {
                action.operation = Operation::Cancel;
                action.target_claim_id = c.target_claim_id.as_ref().map(|id| id.0.clone());
                action.target_item_id = c.target_item_id.as_ref().map(|id| id.0.clone());
                action.quantity = c.quantity;
            }
            _ => continue,
        }
        result.push(action);
    }
    result
}
