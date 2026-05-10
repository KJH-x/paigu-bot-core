use std::sync::Arc;

use crate::domain::ids::RoundId;
use crate::domain::item::Item;
use crate::repo::round_repo::ItemRepo;
use crate::error::AppResult;

pub struct ClaimService {
    pub item_repo: Arc<dyn ItemRepo>,
}

impl ClaimService {
    pub fn new(item_repo: Arc<dyn ItemRepo>) -> Self {
        Self { item_repo }
    }

    pub async fn get_items_for_round(&self, round_id: &RoundId) -> AppResult<Vec<Item>> {
        self.item_repo.find_by_round(round_id).await
    }
}
