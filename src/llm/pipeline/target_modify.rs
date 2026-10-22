use crate::bus::{Delegation, DelegationKind, IncomingEvent};
use crate::messages::processing::ProcessingOutcome;
use crate::parser::policy;
use crate::settings::{
    targets::{target_token, MemberTarget, MemberTargets},
    AppConfig,
};

pub(super) fn prepare(cfg: &AppConfig, ev: &mut IncomingEvent) -> Option<ProcessingOutcome> {
    if ev.delegation.is_some() {
        return None;
    }
    let input = target_token(&ev.text);
    let (head, rest) = input
        .split_once(char::is_whitespace)
        .unwrap_or((&input, ""));
    if head != "/改单" {
        if head.starts_with("/改单") {
            return Some(ProcessingOutcome::new(
                "Rejected",
                "改单命令与目标之间需要空格；未按普通订单处理",
                None,
            ));
        }
        return None;
    }
    let candidates = crate::settings::priority_candidates(cfg, &ev.user_id, &ev.nickname);
    if ev.group_id != cfg.round.group_id
        || !policy::group_allowed(&cfg.gateway.whitelist_groups, &ev.group_id)
        || !policy::member_allowed(
            &cfg.gateway.whitelist_members,
            &candidates.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    {
        return Some(ProcessingOutcome::new(
            "Dropped",
            "管理员改单的实际发言人或群不在允许范围",
            None,
        ));
    }
    if !ev.is_admin {
        return Some(ProcessingOutcome::new(
            "Rejected",
            "非管理员不可指定成员改单",
            None,
        ));
    }
    if !cfg.gateway.admin_commands_enabled {
        return Some(ProcessingOutcome::new(
            "Applied",
            "管理员命令已记录（执行开关关闭）",
            None,
        ));
    }
    let Some((target, body)) = rest.trim().split_once(char::is_whitespace) else {
        return Some(ProcessingOutcome::new(
            "NeedConfirm",
            "用法：/改单 <成员称呼或@成员ID> <更改后的商品与数量>",
            None,
        ));
    };
    let body = body.trim();
    if target.trim().is_empty() || body.is_empty() {
        return Some(ProcessingOutcome::new(
            "NeedConfirm",
            "请同时指定群内成员和更改后的商品与数量",
            None,
        ));
    }
    if body.split_whitespace().any(|token| token.starts_with('/')) {
        return Some(ProcessingOutcome::new(
            "Rejected",
            "改单正文不执行嵌套管理命令",
            None,
        ));
    }
    let index = MemberTargets::load(cfg, &ev.group_id);
    let resolved = index.resolve(target.strip_prefix('@').unwrap_or(target));
    let uid = match resolved {
        MemberTarget::Known(uid) => uid,
        other => {
            let reason = match other {
                MemberTarget::Unavailable => "成员来源不可用或群不匹配，未猜测改单目标",
                MemberTarget::Ambiguous(_) => "成员同名，请用@成员ID指定改单目标",
                MemberTarget::Partial(_) => "仅匹配部分成员名，请补完整称呼或@成员ID",
                _ => "未找到指定的群内成员；管理员目标改单不创建群外代号",
            };
            return Some(ProcessingOutcome::new("NeedConfirm", reason, None));
        }
    };
    ev.delegation = Some(Delegation {
        actor_user_id: ev.user_id.clone(),
        actor_nickname: ev.nickname.clone(),
        source_message_id: ev.message_id.clone(),
        source_text: ev.text.clone(),
        target: DelegationKind::AdminModify { body: body.into() },
    });
    ev.user_id = uid;
    ev.nickname.clear();
    ev.sender_user_id = None;
    ev.sender_nickname = None;
    None
}
