use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use crate::domain::ids::UserId;
use crate::domain::snapshot::AllocationSnapshot;
use crate::engine::replay::rebuild_allocation_snapshot;

use super::Pipeline;

impl Pipeline {
    /// Read-only history of the active pipeline: use recorded events, never reparse messages.
    pub async fn replay_view(&self) -> anyhow::Result<Value> {
        let cfg = self.cfg.get().await;
        let (events, messages, eligibilities, names, version, live) = {
            let state = self.state.lock().await;
            let names: HashMap<UserId, String> = state
                .display
                .iter()
                .filter_map(|(uid, raw)| {
                    crate::settings::resolve_cn(&cfg, uid, raw)
                        .map(|name| (UserId(uid.clone()), name))
                })
                .collect();
            let messages: Vec<Value> = state
                .messages
                .iter()
                .filter(|m| m.status != "Dropped")
                .map(|m| {
                    json!({"seq": m.seq, "display": m.display, "text": m.text,
                    "status": m.status, "detail": m.detail})
                })
                .collect();
            (
                state.events.clone(),
                messages,
                state.eligibilities.clone(),
                names,
                state.version,
                state.snapshot.clone(),
            )
        };
        anyhow::ensure!(
            events
                .iter()
                .all(|event| event.round_id.0 == cfg.round.round_id)
                && live
                    .as_ref()
                    .is_none_or(|board| board.round_id.0 == cfg.round.round_id),
            "轮次正在切换，请稍后刷新"
        );
        let items = cfg.round.to_items();
        let mut by_seq = BTreeMap::<i64, Vec<_>>::new();
        for event in events {
            by_seq.entry(event.sequence).or_default().push(event);
        }
        let mut prefix = Vec::new();
        let mut steps = Vec::new();
        let mut ordered = messages.clone();
        ordered.sort_by_key(|message| message["seq"].as_i64().unwrap_or(0));
        let mut board = rebuild_allocation_snapshot(&items, &prefix, &eligibilities, &names);
        let mut pending = by_seq.into_iter().peekable();
        for message in &ordered {
            let seq = message["seq"].as_i64().unwrap_or(0);
            let mut changed = false;
            while pending
                .peek()
                .is_some_and(|(event_seq, _)| *event_seq <= seq)
            {
                if let Some((_, events)) = pending.next() {
                    prefix.extend(events);
                    changed = true;
                }
            }
            if changed {
                board = rebuild_allocation_snapshot(&items, &prefix, &eligibilities, &names);
            }
            steps.push(
                json!({"index": steps.len(), "message_seq": seq, "board": board,
                "who_whats": who_whats(&board)}),
            );
        }
        if let (Some(last), Some(live)) = (steps.last_mut(), live.as_ref()) {
            last["board"] = serde_json::to_value(live)?;
            last["who_whats"] = json!(who_whats(live));
        }
        let current = self.cfg.get().await;
        anyhow::ensure!(
            current.revision == cfg.revision && current.round.round_id == cfg.round.round_id,
            "配置在读取期间发生变化，请刷新"
        );
        Ok(json!({
            "round_id": cfg.round.round_id, "title": cfg.round.title,
            "revision": cfg.revision, "version": version,
            "items": cfg.round.items, "messages": ordered, "steps": steps,
            "board": live.as_ref().unwrap_or(&board),
            "who_whats": who_whats(live.as_ref().unwrap_or(&board)),
            "source": "active_round", "message_count": messages.len(),
        }))
    }
}

fn who_whats(board: &AllocationSnapshot) -> Vec<Value> {
    board
        .user_summaries
        .iter()
        .map(|summary| {
            json!({"display": summary.display_name, "identity": summary.user_id.0,
            "items": summary.items.iter().map(|item| json!({"name": item.item_name,
                "qty": item.quantity})).collect::<Vec<_>>()})
        })
        .collect()
}
