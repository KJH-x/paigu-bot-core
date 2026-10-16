use super::Pipeline;
use serde_json::Value;

pub(super) struct ViewClock {
    epoch: String,
    revision: u64,
    signature: String,
    board: Option<Value>,
}
impl ViewClock {
    pub(super) fn new() -> Self {
        Self {
            epoch: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            signature: String::new(),
            board: None,
        }
    }
}
impl Pipeline {
    /// Caller holds the common writer and configuration read guard. Event count is
    /// deliberately not used: notes, permissions and quantities can change it by zero.
    pub(crate) async fn view_stamp(
        &self,
        revision: u64,
        board: &Value,
        messages: &[Value],
    ) -> (String, u64, Option<Value>) {
        let signature = crate::messages::transactions::hash(&(revision, board, messages))
            .expect("JSON view is serializable");
        let mut clock = self.view_clock.lock().await;
        let previous = if signature != clock.signature {
            clock.signature = signature;
            clock.revision = clock.revision.saturating_add(1);
            clock.board.replace(board.clone())
        } else {
            None
        };
        (clock.epoch.clone(), clock.revision, previous)
    }
}
