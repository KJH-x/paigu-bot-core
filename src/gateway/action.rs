#![cfg(test)]

use std::sync::Arc;

use serde_json::{json, Value};

use super::ws_server::Gateway;

#[cfg(test)]
pub async fn get_group_member_list(gw: &Arc<Gateway>, group_id: &str) -> anyhow::Result<Value> {
    gw.send_action("get_group_member_list", json!({ "group_id": group_id }))
        .await
}

#[cfg(test)]
pub async fn get_group_info(gw: &Arc<Gateway>, group_id: &str) -> anyhow::Result<Value> {
    gw.send_action("get_group_info", json!({ "group_id": group_id }))
        .await
}

#[cfg(test)]
pub async fn get_group_list(gw: &Arc<Gateway>) -> anyhow::Result<Value> {
    gw.send_action("get_group_list", json!({})).await
}

#[cfg(test)]
pub async fn get_login_info(gw: &Arc<Gateway>) -> anyhow::Result<Value> {
    gw.send_action("get_login_info", json!({})).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use crate::bus::{EventSink, IncomingEvent};
    use crate::settings::ConfigStore;

    struct NullSink;

    #[async_trait]
    impl EventSink for NullSink {
        async fn handle(&self, _ev: IncomingEvent) {}

        fn messages(&self) -> Arc<crate::messages::MessageLog> {
            let dir = std::env::temp_dir().join("paigu-null-sink");
            crate::messages::MessageLog::new(dir.clone(), dir.join("events"))
        }
    }

    fn test_gateway() -> Arc<Gateway> {
        let path = std::env::temp_dir().join(format!(
            "paigu-gateway-action-test-{}.json",
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(ConfigStore::load(path).expect("load test config"));
        Gateway::new(store, Arc::new(NullSink))
    }

    #[tokio::test]
    async fn read_only_helpers_fail_without_client() {
        let gw = test_gateway();
        assert!(get_group_member_list(&gw, "123456789").await.is_err());
        assert!(get_group_info(&gw, "123456789").await.is_err());
        assert!(get_group_list(&gw).await.is_err());
        assert!(get_login_info(&gw).await.is_err());
    }
}
