use std::sync::Arc;
use sqlx::PgPool;

use crate::config::Config;
use crate::error::AppResult;

use crate::repo::postgres::{
    PgRoundRepo, PgItemRepo, PgEventRepo, PgSnapshotRepo,
    PgRawMessageRepo, PgEligibilityRepo, PgUserRepo,
};
use crate::repo::round_repo::{
    RoundRepo, ItemRepo, EventRepo, SnapshotRepo, RawMessageRepo, EligibilityRepo,
};
use crate::repo::user_repo::UserRepo;

use crate::services::round_service::RoundService;
use crate::services::admin_service::AdminService;
use crate::services::message_service::MessageService;
use crate::services::cancel_service::CancelService;
use crate::services::snapshot_service::SnapshotService;
use crate::services::settlement_service::SettlementService;
use crate::services::export_service::ExportService;

use crate::engine::event_store::EventStore;
use crate::engine::event_store::InMemoryEventStore;
use crate::engine::replay::ReplayService;
use crate::parser::parsed_event::MessageParser;
use crate::parser::validation::EventValidator;

pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub repo: Arc<RepoSet>,
    pub services: Arc<ServiceSet>,
}

pub struct RepoSet {
    pub round: Arc<dyn RoundRepo>,
    pub item: Arc<dyn ItemRepo>,
    pub event: Arc<dyn EventRepo>,
    pub snapshot: Arc<dyn SnapshotRepo>,
    pub raw_message: Arc<dyn RawMessageRepo>,
    pub eligibility: Arc<dyn EligibilityRepo>,
    pub user: Arc<dyn UserRepo>,
}

pub struct ServiceSet {
    pub round: Arc<RoundService>,
    pub admin: Arc<AdminService>,
    pub message: Arc<MessageService>,
    pub cancel: Arc<CancelService>,
    pub snapshot: Arc<SnapshotService>,
    pub settlement: Arc<SettlementService>,
    pub export: Arc<ExportService>,
}

impl AppState {
    pub async fn build(config: Config, pool: PgPool) -> AppResult<Self> {
        let round_repo: Arc<dyn RoundRepo> = Arc::new(PgRoundRepo::new(pool.clone()));
        let item_repo: Arc<dyn ItemRepo> = Arc::new(PgItemRepo::new(pool.clone()));
        let event_repo: Arc<dyn EventRepo> = Arc::new(PgEventRepo::new(pool.clone()));
        let snapshot_repo: Arc<dyn SnapshotRepo> = Arc::new(PgSnapshotRepo::new(pool.clone()));
        let raw_message_repo: Arc<dyn RawMessageRepo> = Arc::new(PgRawMessageRepo::new(pool.clone()));
        let eligibility_repo: Arc<dyn EligibilityRepo> = Arc::new(PgEligibilityRepo::new(pool.clone()));
        let user_repo: Arc<dyn UserRepo> = Arc::new(PgUserRepo::new(pool.clone()));

        let event_store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());

        let replay_service = Arc::new(ReplayService::new(event_store.clone()));

        let parser = Arc::new(MessageParser::new(None));

        let round_service = Arc::new(RoundService::new(round_repo.clone()));
        let admin_service = Arc::new(AdminService::new(eligibility_repo.clone(), round_repo.clone(), item_repo.clone(), event_store.clone()));
        let snapshot_service = Arc::new(SnapshotService::new(snapshot_repo.clone(), None));
        let settlement_service = Arc::new(SettlementService::new(snapshot_repo.clone()));
        let export_service = Arc::new(ExportService::new());

        let cancel_service = Arc::new(CancelService::new(event_store.clone()));

        let message_service = Arc::new(MessageService {
            raw_repo: raw_message_repo.clone(),
            round_repo: round_repo.clone(),
            item_repo: item_repo.clone(),
            eligibility_repo: eligibility_repo.clone(),
            event_store: event_store.clone(),
            replay_service: replay_service.clone(),
            snapshot_service: snapshot_service.clone(),
            round_service: round_service.clone(),
            parser: parser.clone(),
            validator: EventValidator::new(config.llm.confidence_threshold),
            pool: Some(pool.clone()),
        });

        let repo = Arc::new(RepoSet {
            round: round_repo,
            item: item_repo,
            event: event_repo,
            snapshot: snapshot_repo,
            raw_message: raw_message_repo,
            eligibility: eligibility_repo,
            user: user_repo,
        });

        let services = Arc::new(ServiceSet {
            round: round_service,
            admin: admin_service,
            message: message_service,
            cancel: cancel_service,
            snapshot: snapshot_service,
            settlement: settlement_service,
            export: export_service,
        });

        Ok(Self { config, pool, repo, services })
    }
}
