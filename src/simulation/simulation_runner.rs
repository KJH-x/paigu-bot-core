use std::sync::Arc;

use crate::parser::parsed_event::MessageParser;
use crate::parser::validation::EventValidator;
use crate::replay::replay_engine::{ReplayEngine, ReplayOptions};
use crate::simulation::simulation_report::SimulationReport;

pub struct SimulationRunner {
    pub parser: Arc<MessageParser>,
    pub validator: EventValidator,
    pub replay_engine: ReplayEngine,
}

impl SimulationRunner {
    pub fn new(parser: Arc<MessageParser>, validator: EventValidator) -> Self {
        Self {
            parser,
            validator,
            replay_engine: ReplayEngine::new(),
        }
    }

    pub async fn run(&self, input: SimulationInput) -> anyhow::Result<SimulationReport> {
        let queue_records = crate::simulation::queue_file::read_jsonl_queue_file(&input.queue_path).await?;

        let mut validated_events = Vec::new();
        let mut parse_failures: Vec<String> = Vec::new();
        let mut validation_failures: Vec<String> = Vec::new();

        for record in queue_records {
            let user_id = crate::domain::ids::UserId(record.user_id.clone());
            let context = crate::parser::llm_client::ParseRequestContext {
                group_id: record.group_id.clone(),
                user_id: user_id.0.clone(),
                nickname: record.nickname.clone(),
                message: record.text.clone(),
                active_rounds: input.round_contexts.clone(),
            };

            let parsed = self.parser.parse_member_message(&context).await;
            match parsed {
                Ok(parsed_msg) => {
                    match self.validator.validate(
                        parsed_msg,
                        &user_id,
                        &record.group_id,
                        Some(record.message_id.clone()),
                        &input.round_contexts,
                        chrono::Utc::now(),
                        (validated_events.len() + 1) as i64,
                    ).await {
                        Ok(outcome) => {
                            match outcome {
                                crate::parser::validation::ValidationOutcome::Ok(event) => {
                                    validated_events.push(event);
                                }
                                crate::parser::validation::ValidationOutcome::Reject(reply) => {
                                    validation_failures.push(format!("Rejected: {:?}", reply));
                                }
                                _ => {
                                    validation_failures.push("Ignored".to_string());
                                }
                            }
                        }
                        Err(e) => {
                            validation_failures.push(e.to_string());
                        }
                    }
                }
                Err(e) => {
                    parse_failures.push(e.to_string());
                }
            }
        }

        let round_config = crate::domain::round::RoundConfig {
            round: crate::domain::round::Round {
                round_id: crate::domain::ids::RoundId(input.round_id.clone()),
                group_id: input.group_id.clone(),
                title: input.title.clone(),
                status: crate::domain::round::RoundStatus::Active,
                start_at: None,
                end_at: None,
                allow_cancel: true,
                allow_modify: true,
                default_timezone: "Asia/Shanghai".to_string(),
                created_by: "simulation".to_string(),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            items: input.items.clone(),
            aliases: vec![],
            eligibility: vec![],
        };

        let replay_result = self.replay_engine.replay(
            round_config,
            validated_events.clone(),
            input.replay_options.clone(),
        ).await.map_err(|e| anyhow::anyhow!("Replay failed: {}", e))?;

        let allocated_count: u64 = replay_result.final_snapshot.user_summaries.iter()
            .flat_map(|s| s.items.iter())
            .map(|i| i.quantity as u64)
            .sum();
        let total_amount = replay_result.final_settlement.as_ref()
            .map(|s| s.final_total.0)
            .unwrap_or(0);
        let allocation_warnings: u64 = replay_result.final_snapshot.warnings.len() as u64;

        Ok(SimulationReport {
            replay_id: input.replay_options.replay_id.clone(),
            round_id: input.round_id.clone(),
            input_message_count: input.total_messages as u64,
            parsed_event_count: validated_events.len() as u64,
            applied_event_count: replay_result.steps.len() as u64,
            parse_failure_count: parse_failures.len() as u64,
            validation_failure_count: validation_failures.len() as u64,
            allocation_warning_count: allocation_warnings,
            final_allocated_claim_count: allocated_count,
            final_unallocated_claim_count: 0,
            final_total_amount: total_amount,
            manifest_path: String::new(),
            final_snapshot_path: String::new(),
            failed_messages_path: String::new(),
            warnings_path: String::new(),
        })
    }
}

pub struct SimulationInput {
    pub round_id: String,
    pub title: String,
    pub group_id: String,
    pub items: Vec<crate::domain::item::Item>,
    pub round_contexts: Vec<crate::domain::item::RoundContext>,
    pub queue_path: String,
    pub replay_options: ReplayOptions,
    pub total_messages: usize,
}
