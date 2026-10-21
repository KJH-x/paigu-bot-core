use serde::Serialize;
use std::path::Path;

use crate::domain::claim::Eligibility;
use crate::domain::item::Item;
use crate::domain::settlement::SettlementSnapshot;
use crate::domain::snapshot::AllocationSnapshot;
use crate::replay::replay_engine::ReplayResult;

mod execute;
mod fixture;
mod report;
pub use execute::verify;
pub(crate) use fixture::load_round_fixture;
use report::write_outputs;

#[derive(Debug, Clone, Serialize)]
pub struct MessageOutcome {
    pub source_sequence: u64,
    pub message_id: String,
    pub user_id: String,
    pub nickname: String,
    pub text: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub round_id: String,
    pub group_id: String,
    pub message_count: usize,
    pub applied_count: usize,
    pub rejected_count: usize,
    pub need_confirm_count: usize,
    pub ignored_count: usize,
    pub unsupported_count: usize,
    pub duplicate_count: usize,
    pub outcomes: Vec<MessageOutcome>,
    pub eligibility: Vec<Eligibility>,
    pub final_snapshot: AllocationSnapshot,
    pub final_settlement: Option<SettlementSnapshot>,
    pub replay: ReplayResult,
}

pub struct RoundFixture {
    pub round_id: String,
    pub title: String,
    pub group_id: String,
    pub items: Vec<Item>,
    /// 预存(购物金)用户；在 priority_window 内仅他们可排。
    pub priority_users: Vec<String>,
    /// 优先时段 [start_ms, end_ms)（end 独占）。
    pub priority_window: Option<(i64, i64)>,
}

pub async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let mut round_config: Option<String> = None;
    let mut queue: Option<String> = None;
    let mut out_dir: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--round-config" => {
                i += 1;
                round_config = args.get(i).cloned();
            }
            "--queue" => {
                i += 1;
                queue = args.get(i).cloned();
            }
            "--out" => {
                i += 1;
                out_dir = args.get(i).cloned();
            }
            other => {
                anyhow::bail!("未知参数: {}", other);
            }
        }
        i += 1;
    }

    let round_config = round_config.ok_or_else(|| anyhow::anyhow!("缺少 --round-config"))?;
    let queue = queue.ok_or_else(|| anyhow::anyhow!("缺少 --queue"))?;
    let out_dir = out_dir.unwrap_or_else(|| "out".to_string());

    let fixture = load_round_fixture(Path::new(&round_config))?;
    let result = verify(Path::new(&queue), fixture).await?;
    write_outputs(Path::new(&out_dir), &result)?;

    println!(
        "验证完成: {} 条消息, 生效 {} / 拒绝 {} / 需确认 {} / 忽略 {} / 未支持 {} / 重复 {}",
        result.message_count,
        result.applied_count,
        result.rejected_count,
        result.need_confirm_count,
        result.ignored_count,
        result.unsupported_count,
        result.duplicate_count
    );
    println!("输出目录: {}", out_dir);
    Ok(())
}

#[cfg(test)]
mod tests;
