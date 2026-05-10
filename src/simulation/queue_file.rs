use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMessageRecord {
    pub source_sequence: u64,
    pub group_id: String,
    pub user_id: String,
    pub nickname: String,
    pub message_id: String,
    pub timestamp_ms: i64,
    pub text: String,
    pub attachments: Vec<MessageAttachment>,
    pub reply_to_message_id: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAttachment {
    pub attachment_type: String,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub sha256: Option<String>,
}

pub async fn read_jsonl_queue_file(path: &str) -> anyhow::Result<Vec<QueueMessageRecord>> {
    let file = tokio::fs::File::open(path).await?;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut records = Vec::new();
    let mut line_no = 0u64;

    while let Some(line) = lines.next_line().await? {
        line_no += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let record: QueueMessageRecord = serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("第 {} 行不是合法 JSON: {}", line_no, e))?;

        records.push(record);
    }

    records.sort_by(|a, b| {
        a.timestamp_ms
            .cmp(&b.timestamp_ms)
            .then_with(|| a.source_sequence.cmp(&b.source_sequence))
            .then_with(|| a.message_id.cmp(&b.message_id))
    });

    Ok(records)
}

impl From<QueueMessageRecord> for crate::repo::round_repo::RawMessageRecord {
    fn from(record: QueueMessageRecord) -> Self {
        let timestamp = chrono::DateTime::from_timestamp_millis(record.timestamp_ms)
            .unwrap_or_else(|| chrono::Utc::now());
        crate::repo::round_repo::RawMessageRecord {
            raw_message_id: record.message_id.clone(),
            group_id: record.group_id,
            user_id: record.user_id,
            qq_message_id: record.message_id,
            timestamp,
            text: Some(record.text),
            images: serde_json::json!(record.attachments),
            is_admin: record.is_admin,
        }
    }
}
