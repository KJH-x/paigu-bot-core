use std::sync::Arc;
use chrono::Utc;

use crate::domain::ids::{UserId, RoundId};
use crate::domain::round::{Round, RoundStatus};
use crate::domain::claim::Eligibility;
use crate::domain::event::{EventEnvelope, DomainEvent, ClaimCreated, EventStatus, AdminAllocationAdjusted, AdminAllocationAction, AdminSlotLocked, DiscountRulesSet};
use crate::domain::item::{Item, RoundContext, ItemKind};
use crate::domain::money::MoneyCents;
use crate::domain::snapshot::AllocationSnapshot;
use crate::inbound::qq_message::IncomingQqMessage;
use crate::inbound::intake;
use crate::inbound::command_router::{self, BotReply, CommandIntent};
use crate::parser::parsed_event::MessageParser;
use crate::parser::validation::{EventValidator, ValidationOutcome};
use crate::repo::round_repo::{RawMessageRepo, RoundRepo, ItemRepo, EventRepo, SnapshotRepo, EligibilityRepo, RawMessageRecord};
use crate::engine::event_store::EventStore;
use crate::engine::replay::ReplayService;
use crate::services::round_service::RoundService;
use crate::services::snapshot_service::SnapshotService;
use crate::error::{AppResult, AppError};

pub struct MessageService {
    pub raw_repo: Arc<dyn RawMessageRepo>,
    pub round_repo: Arc<dyn RoundRepo>,
    pub item_repo: Arc<dyn ItemRepo>,
    pub eligibility_repo: Arc<dyn EligibilityRepo>,
    pub event_store: Arc<dyn EventStore>,
    pub replay_service: Arc<ReplayService>,
    pub snapshot_service: Arc<SnapshotService>,
    pub round_service: Arc<RoundService>,
    pub parser: Arc<MessageParser>,
    pub validator: EventValidator,
    pub pool: Option<sqlx::PgPool>,
}

impl MessageService {
    pub async fn handle_incoming(&self, msg: IncomingQqMessage) -> AppResult<BotReply> {
        if let Some(existing) = self.raw_repo.find_by_message_id(&msg.group_id, &msg.message_id).await? {
            return Ok(BotReply::text(format!("消息已处理 (幂等): {}", existing.raw_message_id)));
        }

        let raw_record = intake::to_raw_message_record(&msg);
        self.raw_repo.insert_raw_message(&raw_record).await?;

        let intent = command_router::classify_message(&msg);

        match intent {
            CommandIntent::MemberClaim | CommandIntent::MemberCancel => {
                self.handle_member_message(msg, intent).await
            }
            CommandIntent::AdminCreateRound
            | CommandIntent::AdminAddItem
            | CommandIntent::AdminAddEligibility
            | CommandIntent::AdminSetDiscount
            | CommandIntent::AdminLockSlot
            | CommandIntent::AdminCloseRound
            | CommandIntent::AdminExport
            | CommandIntent::AdminFixUser => {
                self.handle_admin_message(msg, intent).await
            }
            _ => {
                let active_rounds = self.round_repo.find_active_by_group(&msg.group_id).await?;
                if active_rounds.is_empty() {
                    return Ok(BotReply::silent());
                }

                self.process_as_claim(msg, active_rounds).await
            }
        }
    }

    async fn handle_member_message(&self, msg: IncomingQqMessage, _intent: CommandIntent) -> AppResult<BotReply> {
        let active_rounds = self.round_repo.find_active_by_group(&msg.group_id).await?;
        if active_rounds.is_empty() {
            return Ok(BotReply::text("当前没有活跃的拼团。"));
        }

        self.process_as_claim(msg, active_rounds).await
    }

    async fn handle_admin_message(&self, msg: IncomingQqMessage, intent: CommandIntent) -> AppResult<BotReply> {
        if !msg.is_admin {
            return Ok(BotReply::AdminOnly("此命令仅管理员可用。".to_string()));
        }

        match intent {
            CommandIntent::AdminCreateRound => {
                let title = msg.text.trim_start_matches('/').trim().to_string();
                if title.is_empty() {
                    return Ok(BotReply::text("用法：/开团 标题=ES5月新谷"));
                }
                let round = self.round_service.create_round(
                    title,
                    msg.group_id.clone(),
                    msg.user_id.clone(),
                    None,
                    None,
                ).await?;
                Ok(BotReply::text(format!("已创建团 {} (ID: {})", round.title, round.round_id.0)))
            }
            CommandIntent::AdminCloseRound => {
                let active_rounds = self.round_service.get_active_rounds(&msg.group_id).await?;
                if active_rounds.is_empty() {
                    return Ok(BotReply::text("当前没有活跃的拼团可关闭。"));
                }
                self.round_service.close_round(&active_rounds[0].round_id).await?;
                Ok(BotReply::text(format!("已结团：{}", active_rounds[0].title)))
            }
            CommandIntent::AdminLockSlot => {
                let active_rounds = self.round_service.get_active_rounds(&msg.group_id).await?;
                if active_rounds.is_empty() {
                    return Ok(BotReply::text("没有活跃的团。"));
                }
                let round_id = &active_rounds[0].round_id;
                let event = EventEnvelope {
                    event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
                    round_id: round_id.clone(),
                    group_id: msg.group_id.clone(),
                    user_id: UserId(msg.user_id.clone()),
                    raw_message_id: Some(msg.message_id.clone()),
                    event_type: "admin_slot_locked".to_string(),
                    effective_at: Utc::now(),
                    sequence: 0,
                    payload: DomainEvent::AdminSlotLocked(AdminSlotLocked {
                        item_id: crate::domain::ids::ItemId("unknown".to_string()),
                        box_index: 1,
                        slot_index: 1,
                        reason: Some("管理员锁位".to_string()),
                        locked_by: UserId(msg.user_id.clone()),
                    }),
                    status: EventStatus::Active,
                };
                self.event_store.append(&event).await?;
                Ok(BotReply::text("已锁位。"))
            }
            CommandIntent::AdminFixUser => {
                let active_rounds = self.round_service.get_active_rounds(&msg.group_id).await?;
                if active_rounds.is_empty() {
                    return Ok(BotReply::text("没有活跃的团。"));
                }
                let round_id = &active_rounds[0].round_id;
                let event = EventEnvelope {
                    event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
                    round_id: round_id.clone(),
                    group_id: msg.group_id.clone(),
                    user_id: UserId(msg.user_id.clone()),
                    raw_message_id: Some(msg.message_id.clone()),
                    event_type: "admin_allocation_adjusted".to_string(),
                    effective_at: Utc::now(),
                    sequence: 0,
                    payload: DomainEvent::AdminAllocationAdjusted(AdminAllocationAdjusted {
                        adjustment_id: uuid::Uuid::new_v4().to_string(),
                        action: AdminAllocationAction::LockSlot {
                            item_id: crate::domain::ids::ItemId("unknown".to_string()),
                            box_index: 1,
                            slot_index: 1,
                            reason: Some("管理员修正".to_string()),
                        },
                        reason: Some("管理员修正".to_string()),
                    }),
                    status: EventStatus::Active,
                };
                self.event_store.append(&event).await?;
                Ok(BotReply::text("已执行管理员修正。"))
            }
            CommandIntent::AdminSetDiscount => {
                let active_rounds = self.round_service.get_active_rounds(&msg.group_id).await?;
                if active_rounds.is_empty() {
                    return Ok(BotReply::text("没有活跃的团。"));
                }
                let round_id = &active_rounds[0].round_id;
                let event = EventEnvelope {
                    event_id: crate::domain::ids::EventId(uuid::Uuid::new_v4().to_string()),
                    round_id: round_id.clone(),
                    group_id: msg.group_id.clone(),
                    user_id: UserId(msg.user_id.clone()),
                    raw_message_id: Some(msg.message_id.clone()),
                    event_type: "discount_rules_set".to_string(),
                    effective_at: Utc::now(),
                    sequence: 0,
                    payload: DomainEvent::DiscountRulesSet(DiscountRulesSet {
                        source_text: msg.text.clone(),
                        rules: vec![],
                    }),
                    status: EventStatus::Active,
                };
                self.event_store.append(&event).await?;
                Ok(BotReply::text("已记录优惠规则。"))
            }
            CommandIntent::AdminExport => {
                let active_rounds = self.round_service.get_active_rounds(&msg.group_id).await?;
                if active_rounds.is_empty() {
                    return Ok(BotReply::text("没有活跃的团。"));
                }
                Ok(BotReply::text("导出功能请调用管理后台 API。"))
            }
            CommandIntent::AdminAddItem => {
                Ok(BotReply::text("请使用 /加商品 名称=... 类型=拼团 单价=... 格式添加商品。"))
            }
            CommandIntent::AdminAddEligibility => {
                Ok(BotReply::text("请使用 /加优先 用户=@A 等级=10 格式添加优先权。"))
            }
            _ => Ok(BotReply::text("管理员命令已记录")),
        }
    }

    async fn process_as_claim(&self, msg: IncomingQqMessage, active_rounds: Vec<Round>) -> AppResult<BotReply> {
        let mut round_contexts: Vec<RoundContext> = Vec::new();
        for r in &active_rounds {
            if !r.status.allows_claims() {
                if let Some(start) = r.start_at {
                    if Utc::now() < start {
                        return Ok(BotReply::text("当前团尚未开始，请等待开团时间。"));
                    }
                }
                return Ok(BotReply::text("当前团未开放排谷。"));
            }
            round_contexts.push(self.load_round_context(r).await);
        }

        let user_id = UserId(msg.user_id.clone());
        let now = Utc::now();

        let context = crate::parser::llm_client::ParseRequestContext {
            group_id: msg.group_id.clone(),
            user_id: user_id.0.clone(),
            nickname: msg.nickname.clone(),
            message: msg.text.clone(),
            active_rounds: round_contexts.clone(),
        };

        let parsed = self.parser.parse_member_message(&context).await;

        match parsed {
            Ok(parsed_msg) => {
                match self.validator.validate(
                    parsed_msg,
                    &user_id,
                    &msg.group_id,
                    Some(msg.message_id.clone()),
                    &round_contexts,
                    now,
                    1,
                ).await? {
                    ValidationOutcome::Ok(event) => {
                        let round_id = event.round_id.clone();
                        self.event_store.append(&event).await?;

                        let round = active_rounds.iter().find(|r| r.round_id == round_id)
                            .or_else(|| active_rounds.first())
                            .ok_or_else(|| AppError::NotFound("Round not found".to_string()))?;

                        let items = self.item_repo.find_by_round(&round.round_id).await?;
                        let eligibility = self.eligibility_repo.find_by_round(&round.round_id).await?;

                        let (snapshot, _) = self.replay_service.rebuild_snapshot(
                            &round.round_id, &items, &eligibility, round
                        ).await?;

                        self.snapshot_service.save_and_publish(
                            &snapshot, &round.title, round.status.as_str()
                        ).await?;

                        Ok(BotReply::text(format!(
                            "已记录，当前版本 #{}", snapshot.version
                        )))
                    }
                    ValidationOutcome::NeedConfirm(reply) => Ok(reply),
                    ValidationOutcome::Reject(reply) => Ok(reply),
                    ValidationOutcome::Ignore => Ok(BotReply::silent()),
                }
            }
            Err(e) => {
                Ok(BotReply::text(format!("解析失败：{}。请按格式重发。", e)))
            }
        }
    }

    async fn load_round_context(&self, round: &Round) -> RoundContext {
        let items = self.item_repo.find_by_round(&round.round_id).await.unwrap_or_default();
        RoundContext {
            round_id: round.round_id.clone(),
            title: round.title.clone(),
            items,
        }
    }
}
