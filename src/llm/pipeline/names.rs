use crate::domain::ids::UserId;
use crate::messages::MessageRecord;
use crate::parser::policy;
use crate::settings::AppConfig;

use super::state::State;

impl State {
    pub(super) fn remember_name(
        &mut self,
        cfg: &AppConfig,
        record: &MessageRecord,
        display: &str,
        status: &str,
    ) -> bool {
        if matches!(status, "Dropped" | "Duplicate")
            || record.routed.to_ascii_lowercase().starts_with("drop")
            || !policy::group_allowed(&cfg.gateway.whitelist_groups, &record.group_id)
        {
            return false;
        }
        let (actor, nick) = record
            .delegation
            .as_ref()
            .map(|d| (d.actor_user_id.as_str(), d.actor_nickname.as_str()))
            .unwrap_or((
                record.sender_user_id.as_deref().unwrap_or(&record.user_id),
                record
                    .sender_nickname
                    .as_deref()
                    .unwrap_or(&record.nickname),
            ));
        let candidates = crate::settings::priority_candidates(cfg, actor, nick);
        if !policy::member_allowed(
            &cfg.gateway.whitelist_members,
            &candidates.iter().map(String::as_str).collect::<Vec<_>>(),
        ) {
            return false;
        }
        let changed = self
            .display
            .get(&record.user_id)
            .is_none_or(|name| name != display);
        self.display.insert(record.user_id.clone(), display.into());
        self.identity.insert(
            record.user_id.clone(),
            crate::parser::normalize::clean_nickname(&record.nickname).0,
        );
        changed
    }

    pub(super) fn refresh_snapshot_names(&mut self, cfg: &AppConfig) {
        if self.snapshot.is_none() {
            return;
        }
        let names = self
            .display
            .iter()
            .filter_map(|(uid, name)| {
                crate::settings::resolve_cn(cfg, uid, name).map(|name| (UserId(uid.clone()), name))
            })
            .collect();
        self.snapshot = Some(crate::engine::replay::rebuild_allocation_snapshot(
            &cfg.round.to_items(),
            &self.events,
            &self.eligibilities,
            &names,
        ));
    }
}
