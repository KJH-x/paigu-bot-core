use chrono::{DateTime, Utc};

pub type Timestamp = DateTime<Utc>;

pub fn utc_now() -> Timestamp {
    Utc::now()
}

pub fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}
