use super::{prepare, replay_report, MessageOutcome, RoundFixture, VerifyResult};
use crate::parser::parsed_event::ParseOverrides;
use crate::replay::session;
use crate::simulation::queue_file::read_jsonl_queue_file;
use std::path::Path;

pub async fn verify(queue_path: &Path, fixture: RoundFixture) -> anyhow::Result<VerifyResult> {
    let queue = read_jsonl_queue_file(
        queue_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("夹具路径不是有效Unicode"))?,
    )
    .await?;
    let (records, eligibility) = prepare::prepare(&queue, &fixture).await?;
    let current = session::replay_prepared(
        &fixture.config,
        &records,
        &fixture.items,
        &eligibility,
        &ParseOverrides::default(),
    )
    .await?;
    let outcomes: Vec<_> = queue
        .iter()
        .zip(&records)
        .map(|(input, record)| {
            let view = current
                .messages
                .iter()
                .find(|view| view.record_id == record.record_id)
                .ok_or_else(|| anyhow::anyhow!("离线会话缺少原消息投影"))?;
            Ok(MessageOutcome {
                source_sequence: input.source_sequence,
                message_id: input.message_id.clone(),
                user_id: input.user_id.clone(),
                nickname: view.nickname.clone(),
                text: input.text.clone(),
                status: view.status.clone(),
                detail: view.detail.clone(),
            })
        })
        .collect::<anyhow::Result<_>>()?;
    let replay = replay_report::build(&fixture, &records, &current);
    let count = |status: &str| outcomes.iter().filter(|o| o.status == status).count();
    Ok(VerifyResult {
        round_id: fixture.config.round.round_id.clone(),
        group_id: fixture.config.round.group_id.clone(),
        message_count: queue.len(),
        applied_count: count("Applied"),
        rejected_count: count("Rejected"),
        need_confirm_count: count("NeedConfirm"),
        ignored_count: count("Ignored"),
        unsupported_count: count("Unsupported"),
        duplicate_count: count("Duplicate"),
        dropped_count: count("Dropped"),
        merged_count: count("Merged"),
        error_count: count("Error"),
        final_snapshot: current.board.clone(),
        final_settlement: replay.final_settlement.clone(),
        eligibility: current.eligibilities,
        outcomes,
        replay,
    })
}
