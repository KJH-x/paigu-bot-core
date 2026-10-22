use super::Pipeline;
use crate::bus::{Delegation, DelegationKind, IncomingEvent, PipelineOutcome};
use crate::messages::processing::ProcessingOutcome;
use crate::settings::targets::{target_token, MemberTarget, MemberTargets};
use crate::settings::AppConfig;

pub(super) struct Segment {
    pub target: Option<String>,
    pub resolved: Option<MemberTarget>,
    pub text: String,
    pub issue: Option<&'static str>,
}

pub(super) fn segments(cfg: &AppConfig, ev: &IncomingEvent) -> Option<Vec<Segment>> {
    if ev.delegation.is_some() || ev.text.trim_start().starts_with('/') {
        return None;
    }
    let index = MemberTargets::load(cfg, &ev.group_id);
    let mut markers = find_proxy_markers(&ev.text);
    let mut offset = 0;
    let mut conflicts = std::collections::BTreeSet::new();
    for line in ev.text.split_inclusive('\n') {
        if let Some(colon) = line.find([':', '：']) {
            let name = target_token(&line[..colon]);
            if !name.is_empty()
                && name.chars().count() <= 24
                && (name.starts_with('@')
                    || matches!(
                        index.resolve(&name),
                        MemberTarget::Known(_)
                            | MemberTarget::Ambiguous(_)
                            | MemberTarget::Partial(_)
                    ))
                && !markers
                    .iter()
                    .any(|(_, start, end)| offset >= *start && offset < *end)
            {
                let collision =
                    cfg.round.items.iter().any(|item| {
                        std::iter::once(&item.name)
                            .chain(item.aliases.iter())
                            .chain(item.subitems.iter().flat_map(|sub| {
                                std::iter::once(&sub.name).chain(sub.aliases.iter())
                            }))
                            .any(|label| target_token(label) == name)
                    });
                if collision {
                    conflicts.insert(offset);
                }
                let separator_len = line[colon..].chars().next().map_or(0, char::len_utf8);
                markers.push((name, offset, offset + colon + separator_len));
            }
        }
        offset += line.len();
    }
    markers.sort_by_key(|(_, start, _)| *start);
    if markers.is_empty() {
        return None;
    }
    let mut result = vec![];
    let mut pos = 0;
    let mut current = None;
    let mut issue = None;
    for (target, start, end) in markers {
        if current.is_some() || (start > pos && !ev.text[pos..start].trim().is_empty()) {
            result.push(Segment {
                target: current,
                resolved: None,
                text: ev.text[pos..start].into(),
                issue,
            });
        }
        pos = end;
        current = Some(target);
        issue = conflicts
            .contains(&start)
            .then_some("名字与商品标题相同，请使用明确代排或@成员标识");
    }
    if pos <= ev.text.len() {
        result.push(Segment {
            target: current,
            resolved: None,
            text: ev.text[pos..].into(),
            issue,
        });
    }
    for part in &mut result {
        part.resolved = part.target.as_ref().map(|target| {
            let token = target_token(target);
            if !token.starts_with('@')
                && (token.chars().all(|c| c.is_ascii_digit()) || token == "群外")
            {
                MemberTarget::Unknown
            } else {
                index.resolve(token.strip_prefix('@').unwrap_or(&token))
            }
        });
    }
    Some(result)
}

pub(crate) fn assign_guest(cfg: &AppConfig, ev: &mut IncomingEvent, label: Option<String>) {
    let delegation = Delegation {
        actor_user_id: ev.user_id.clone(),
        actor_nickname: ev.nickname.clone(),
        source_message_id: ev.message_id.clone(),
        source_text: ev.text.clone(),
        target: DelegationKind::Guest { label },
    };
    ev.sender_user_id = Some(ev.user_id.clone());
    ev.sender_nickname = Some(ev.nickname.clone());
    ev.user_id = delegation.guest_id(&ev.group_id);
    ev.nickname =
        crate::settings::delegation_display(cfg, &delegation).unwrap_or_else(|| ev.user_id.clone());
    ev.delegation = Some(delegation);
}

impl Pipeline {
    pub(super) async fn legacy_guest_owner(
        &self,
        cfg: &AppConfig,
        ev: &mut IncomingEvent,
    ) -> Option<ProcessingOutcome> {
        let meta = ev.delegation.as_ref()?;
        let DelegationKind::Guest { label } = &meta.target else {
            return None;
        };
        let records = match self
            .messages
            .read_ingress_records(&cfg.round.round_id)
            .await
        {
            Ok(records) => records,
            Err(_) => {
                return Some(ProcessingOutcome::new(
                    "Error",
                    "旧代排记录不可完整读取，未猜测归属",
                    None,
                ))
            }
        };
        let mut owners = std::collections::BTreeSet::new();
        let mut uncertain = false;
        for record in &records {
            if record.delegation.is_some() || record.group_id != ev.group_id {
                continue;
            }
            let Some((_, suffix)) = record.user_id.rsplit_once("(代") else {
                continue;
            };
            let Some(old_label) = suffix.strip_suffix(')') else {
                continue;
            };
            if target_token(old_label) != label.as_deref().unwrap_or("") {
                continue;
            }
            match record.sender_user_id.as_deref() {
                Some(actor) if actor == meta.actor_user_id => {
                    owners.insert(record.user_id.clone());
                }
                None => uncertain = true,
                _ => {}
            }
        }
        if records.iter().any(|record| {
            owners.contains(&record.user_id)
                && record
                    .sender_user_id
                    .as_deref()
                    .is_some_and(|actor| actor != meta.actor_user_id)
        }) {
            uncertain = true;
        }
        if uncertain || owners.len() > 1 {
            return Some(ProcessingOutcome::new(
                "NeedConfirm",
                "旧代排缺少唯一发言人或身份冲突，请先核对旧订单；未新建另一身份",
                None,
            ));
        }
        if let Some(owner) = owners.into_iter().next() {
            ev.user_id = owner;
        }
        None
    }

    pub(super) async fn process_proxy_segments(
        &self,
        ev: &IncomingEvent,
        parts: Vec<Segment>,
    ) -> PipelineOutcome {
        let cfg = self.cfg.get().await;
        let mut statuses = vec![];
        let mut details = vec![];
        let mut version = 0;
        let mut snapshot = None;
        for (idx, part) in parts.into_iter().enumerate() {
            let mut seg = ev.clone();
            seg.text = part.text;
            seg.message_id = format!("{}#proxy{idx}", ev.message_id);
            seg.ingress = ev.ingress.as_ref().map(|s| crate::bus::IngressSource {
                id: s.id.clone(),
                part: format!("{}:proxy-{idx}", s.part),
            });
            seg.raw = None;
            let mut forced = part
                .issue
                .map(|issue| ProcessingOutcome::new("NeedConfirm", issue, None));
            if let Some(target) = part.target {
                let token = target_token(&target);
                let explicit = token.strip_prefix('@');
                let resolution = part.resolved.unwrap_or(MemberTarget::Unavailable);
                match resolution {
                    MemberTarget::Known(uid) => {
                        seg.user_id = uid;
                        seg.nickname.clear();
                        seg.delegation = Some(Delegation {
                            actor_user_id: ev.user_id.clone(),
                            actor_nickname: ev.nickname.clone(),
                            source_message_id: ev.message_id.clone(),
                            source_text: ev.text.clone(),
                            target: DelegationKind::Member,
                        });
                    }
                    MemberTarget::Unknown if explicit.is_none() => {
                        assign_guest(&cfg, &mut seg, Some(token));
                        if let Some(meta) = &mut seg.delegation {
                            meta.source_message_id = ev.message_id.clone();
                            meta.source_text = ev.text.clone();
                        }
                    }
                    other => {
                        let reason = match other {
                            MemberTarget::Unavailable => "成员来源不可用或群不匹配，未猜测归属",
                            MemberTarget::Ambiguous(_) => "成员同名，请使用@成员标识消歧",
                            MemberTarget::Partial(_) => {
                                "只匹配到部分成员名，请补充完整称呼或@成员标识"
                            }
                            _ => "未找到明确指定的群内成员",
                        };
                        forced = Some(ProcessingOutcome::new("NeedConfirm", reason, None));
                        seg.text = ev.text.clone();
                    }
                }
            }
            if seg.text.trim().is_empty() {
                forced = Some(ProcessingOutcome::new(
                    "NeedConfirm",
                    "代排标记后缺少动作",
                    None,
                ));
            } else if seg.text.trim_start().starts_with('/') {
                forced = Some(ProcessingOutcome::new(
                    "Rejected",
                    "代排段内不执行管理命令",
                    None,
                ));
            }
            let outcome = Box::pin(self.process_one_inner(seg, forced)).await;
            statuses.push(outcome.status.clone());
            details.push(format!("{} [{}]", outcome.detail, outcome.status));
            version = outcome.version;
            if outcome.snapshot.is_some() {
                snapshot = outcome.snapshot;
            }
            if outcome.status == "Error" {
                break;
            }
        }
        let status = ["Error", "NeedConfirm", "Rejected", "Applied"]
            .into_iter()
            .find(|status| statuses.iter().any(|s| s == status))
            .map(str::to_string)
            .unwrap_or_else(|| statuses.last().cloned().unwrap_or_else(|| "Ignored".into()));
        PipelineOutcome {
            status,
            detail: format!("代排：{}", details.join("；")),
            reply: None,
            version,
            snapshot,
        }
    }
}

fn find_proxy_markers(text: &str) -> Vec<(String, usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1 == '代' {
            let mut j = i + 1;
            while j < chars.len()
                && !matches!(
                    chars[j].1,
                    '排' | ' ' | ':' | '：' | '\n' | '\t' | '，' | ','
                )
                && j - i <= 24
            {
                j += 1;
            }
            if j < chars.len() && chars[j].1 == '排' && j > i + 1 {
                let name: String = chars[i + 1..j].iter().map(|(_, c)| *c).collect();
                let start = chars[i].0;
                let mut end = chars[j].0 + chars[j].1.len_utf8();
                if let Some(c) = text[end..].chars().next() {
                    if c == ':' || c == '：' {
                        end += c.len_utf8();
                    }
                }
                out.push((name, start, end));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}
