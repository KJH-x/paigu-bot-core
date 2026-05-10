use crate::inbound::qq_message::IncomingQqMessage;
use crate::repo::round_repo::RawMessageRecord;

pub fn to_raw_message_record(msg: &IncomingQqMessage) -> RawMessageRecord {
    let timestamp = msg.timestamp;
    RawMessageRecord {
        raw_message_id: uuid::Uuid::new_v4().to_string(),
        group_id: msg.group_id.clone(),
        user_id: msg.user_id.clone(),
        qq_message_id: msg.message_id.clone(),
        timestamp,
        text: Some(msg.text.clone()),
        images: serde_json::Value::Array(vec![]),
        is_admin: msg.is_admin,
    }
}
