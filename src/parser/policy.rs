//! 排谷策略/判定共享模块（D-02）。
//!
//! 实时（`llm::pipeline::process`）、重放（`replay::session::process_one`）与
//! 离线模拟（`simulation::verifier::verify`）原先各自复制了同一套「白名单匹配 /
//! 优先时段 / 阶段权限」逻辑。此处收敛为单一实现，保证「实时 == 重放」不漂移。
//!
//! 语义与收敛前逐字一致：
//! - 白名单：群名单为空视为不限制；成员名单为空视为不限制，命中任一候选串即放行。
//! - 优先时段：`[start_ms, end_ms)`（end 独占）；优先用户命中任一候选串即可。
//! - 阶段：复用 `round::{phase_at, can_claim, can_cancel}` 的权限矩阵。

use crate::domain::event::{DomainEvent, EventEnvelope};
use crate::round::{can_cancel, can_claim, RoundPhase};
use crate::settings::AppConfig;

pub use crate::round::phase_at;
pub use crate::settings::{in_priority_window, is_priority_user};

/// 群白名单：命中 `group_id` 才放行（空名单不放行任何群，与网关路由一致）。
pub fn group_allowed(whitelist_groups: &[String], group_id: &str) -> bool {
    whitelist_groups.iter().any(|g| g == group_id)
}

/// 成员白名单：空名单 = 不限制；否则任一「去空白、非空」条目命中候选串即放行。
pub fn member_allowed(whitelist_members: &[String], candidates: &[&str]) -> bool {
    if whitelist_members.is_empty() {
        return true;
    }
    whitelist_members.iter().any(|m| {
        let m = m.trim();
        !m.is_empty() && candidates.contains(&m)
    })
}

/// 从配置提取优先时段 `[start_ms, end_ms)`；`None` = 无时段限制。
pub fn priority_window(cfg: &AppConfig) -> Option<(i64, i64)> {
    cfg.round
        .priority_window
        .as_ref()
        .map(|w| (w.start_ms, w.end_ms))
}

/// 优先用户判定：候选串为 `[user_id, nickname, identity, display]`。
pub fn is_priority(cfg: &AppConfig, candidates: &[&str]) -> bool {
    is_priority_user(&cfg.round.priority_users, candidates)
}

/// 当前时间是否落在优先时段内。
pub fn in_priority_at(cfg: &AppConfig, timestamp_ms: i64) -> bool {
    in_priority_window(priority_window(cfg), timestamp_ms)
}

pub fn phase_label(phase: RoundPhase) -> &'static str {
    match phase {
        RoundPhase::Phase0 => "Phase 0",
        RoundPhase::PhaseI => "Phase I",
        RoundPhase::PhaseII => "Phase II",
        RoundPhase::PhaseIII => "Phase III",
        RoundPhase::Settling => "结算",
        RoundPhase::Locked => "锁定",
    }
}

/// 阶段越权判定；返回 `Some(detail)` 表示应拒绝（detail 含「阶段」）。
pub fn phase_rejection(
    cfg: &AppConfig,
    event: &EventEnvelope,
    phase: RoundPhase,
    is_priority: bool,
) -> Option<String> {
    let label = phase_label(phase);
    if phase == RoundPhase::Locked {
        return Some(format!("阶段越权：{label}阶段已锁定，禁止操作"));
    }
    match &event.payload {
        DomainEvent::ClaimCreated(c) => c.items.iter().find_map(|line| {
            let class = cfg.round.item_class(&line.item_id.0);
            (!can_claim(phase, class, is_priority))
                .then_some(format!("阶段越权：{label}阶段不允许排该商品"))
        }),
        DomainEvent::ClaimCancelled(c) => c.target_item_id.as_ref().and_then(|item_id| {
            let class = cfg.round.item_class(&item_id.0);
            (!can_cancel(phase, class, is_priority))
                .then_some(format!("阶段越权：{label}阶段不允许撤销该商品"))
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::round::{ItemClass, PhaseWindow};

    fn cfg_with(mutate: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut cfg = crate::settings::default_config();
        mutate(&mut cfg);
        cfg
    }

    #[test]
    fn group_whitelist_requires_exact_match() {
        let wl = vec!["123456789".to_string()];
        assert!(group_allowed(&wl, "123456789"));
        assert!(!group_allowed(&wl, "123"));
        assert!(!group_allowed(&[], "123456789"));
    }

    #[test]
    fn member_whitelist_empty_allows_all() {
        assert!(member_allowed(&[], &["u1"]));
    }

    #[test]
    fn member_whitelist_matches_any_trimmed_candidate() {
        let wl = vec![" 甲 ".to_string()];
        assert!(member_allowed(&wl, &["u1", "甲", "甲(代乙)"]));
        assert!(!member_allowed(&wl, &["u1", "乙"]));
        let blank = vec!["   ".to_string()];
        assert!(!member_allowed(&blank, &["u1"]));
    }

    #[test]
    fn priority_window_is_half_open() {
        let cfg = cfg_with(|c| {
            c.round.priority_window = Some(crate::settings::PriorityWindow {
                start_ms: 100,
                end_ms: 200,
            });
        });
        assert_eq!(priority_window(&cfg), Some((100, 200)));
        assert!(!in_priority_at(&cfg, 99));
        assert!(in_priority_at(&cfg, 100));
        assert!(in_priority_at(&cfg, 199));
        assert!(!in_priority_at(&cfg, 200));
    }

    #[test]
    fn is_priority_matches_any_candidate() {
        let cfg = cfg_with(|c| c.round.priority_users = vec!["u2".to_string()]);
        assert!(is_priority(&cfg, &["u1", "u2", "u2", "u2"]));
        assert!(!is_priority(&cfg, &["u1", "u1", "u1", "u1"]));
    }

    #[test]
    fn phase_labels_are_stable() {
        assert_eq!(phase_label(RoundPhase::Phase0), "Phase 0");
        assert_eq!(phase_label(RoundPhase::Settling), "结算");
        assert_eq!(phase_label(RoundPhase::Locked), "锁定");
    }

    #[test]
    fn phase_locked_rejects_any_operation() {
        let cfg = cfg_with(|c| {
            c.round.phases = vec![PhaseWindow {
                phase: RoundPhase::Locked,
                start_ms: 0,
                end_ms: i64::MAX,
            }];
        });
        let phase = phase_at(&cfg.round.phases, 1).unwrap();
        assert_eq!(phase, RoundPhase::Locked);
        let event = cancelled_event("x");
        assert!(phase_rejection(&cfg, &event, phase, true)
            .unwrap()
            .contains("阶段"));
    }

    fn cancelled_event(item_id: &str) -> EventEnvelope {
        use crate::domain::ids::{ItemId, RoundId, UserId};
        EventEnvelope {
            event_id: crate::domain::ids::EventId("e".to_string()),
            round_id: RoundId("r".to_string()),
            group_id: "g".to_string(),
            user_id: UserId("u".to_string()),
            raw_message_id: None,
            event_type: "claim_cancelled".to_string(),
            effective_at: chrono::Utc::now(),
            sequence: 1,
            payload: DomainEvent::ClaimCancelled(crate::domain::event::ClaimCancelled {
                target_claim_id: None,
                target_item_id: Some(ItemId(item_id.to_string())),
                quantity: None,
                reason: None,
                parse_trace: None,
                validation_trace: vec![],
            }),
            status: crate::domain::event::EventStatus::Active,
        }
    }

    #[test]
    fn phase_ii_class_a_requires_priority() {
        let cfg = cfg_with(|c| {
            c.round.phases = vec![PhaseWindow {
                phase: RoundPhase::PhaseII,
                start_ms: 0,
                end_ms: i64::MAX,
            }];
            for it in &mut c.round.items {
                if it.item_id == "pass_sp" {
                    it.class = Some("A".to_string());
                }
            }
        });
        assert_eq!(cfg.round.item_class("pass_sp"), ItemClass::A);
        let event = cancelled_event("pass_sp");
        let phase = RoundPhase::PhaseII;
        assert!(phase_rejection(&cfg, &event, phase, false).is_some());
        assert!(phase_rejection(&cfg, &event, phase, true).is_none());
    }
}
