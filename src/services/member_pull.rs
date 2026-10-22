//! Process-local health for read-only member pulls; never a second member cache.
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, TimeZone};
use serde::Serialize;
use serde_json::{json, Value};

use super::ServiceError;
use crate::{api::ApiState, settings::AppConfig};

#[cfg(test)]
#[path = "member_pull_tests.rs"]
mod tests;

#[derive(Clone, PartialEq, Eq)]
struct Source {
    round: String,
    group: String,
    cache: String,
}
impl Source {
    fn from_config(cfg: &AppConfig) -> Self {
        Self {
            round: cfg.round.round_id.clone(),
            group: cfg.members.group_id.clone(),
            cache: cfg.members.cache_path.clone(),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Manual,
    Scheduled,
}

#[derive(Clone, Serialize)]
struct AttemptView {
    #[serde(skip)]
    source: Source,
    #[serde(skip)]
    id: u64,
    origin: Origin,
    started_at_ms: i64,
    finished_at_ms: Option<i64>,
    status: &'static str,
    reason: Option<&'static str>,
    member_count: Option<usize>,
}

#[derive(Default)]
struct State {
    serial: u64,
    active: Option<AttemptView>,
    last: Option<AttemptView>,
    success: Option<(Source, i64)>,
    failures: Option<(Source, u32)>,
    plan: Option<(Source, String, Option<i64>)>,
}

#[derive(Default)]
pub struct Monitor {
    state: Mutex<State>,
}

pub struct Attempt {
    monitor: Arc<Monitor>,
    id: u64,
    finished: bool,
}

impl Monitor {
    pub fn begin(
        self: &Arc<Self>,
        cfg: &AppConfig,
        origin: Origin,
    ) -> Result<Attempt, ServiceError> {
        let mut state = self.state.lock().unwrap();
        if state.active.is_some() {
            return Err(ServiceError::Conflict(
                json!({"error":"members_pull_busy","message":"已有成员拉取进行中，未叠加请求；请等待或检查接收器"}),
            ));
        }
        state.serial = state.serial.saturating_add(1);
        let id = state.serial;
        state.active = Some(AttemptView {
            source: Source::from_config(cfg),
            id,
            origin,
            started_at_ms: super::messages::now_ms(),
            finished_at_ms: None,
            status: "running",
            reason: None,
            member_count: None,
        });
        Ok(Attempt {
            monitor: self.clone(),
            id,
            finished: false,
        })
    }

    fn complete(
        &self,
        id: u64,
        status: &'static str,
        reason: Option<&'static str>,
        count: Option<usize>,
    ) {
        let mut state = self.state.lock().unwrap();
        if state.active.as_ref().is_none_or(|active| active.id != id) {
            return;
        }
        let mut active = state.active.take().unwrap();
        let finished = super::messages::now_ms();
        active.finished_at_ms = Some(finished);
        active.status = status;
        active.reason = reason;
        active.member_count = count;
        if status == "succeeded" {
            state.success = Some((active.source.clone(), finished));
            state.failures = Some((active.source.clone(), 0));
        } else {
            let previous = state
                .failures
                .as_ref()
                .filter(|(source, _)| source == &active.source)
                .map(|(_, count)| *count)
                .unwrap_or(0);
            state.failures = Some((active.source.clone(), previous.saturating_add(1)));
        }
        state.last = Some(active);
    }

    pub fn snapshot(&self, cfg: &AppConfig) -> Value {
        let source = Source::from_config(cfg);
        let state = self.state.lock().unwrap();
        let plan = state
            .plan
            .as_ref()
            .filter(|(scope, at, _)| scope == &source && at == &cfg.members.daily_pull_at);
        let attempt = state
            .active
            .as_ref()
            .filter(|a| a.source == source)
            .or_else(|| state.last.as_ref().filter(|a| a.source == source));
        json!({"running":state.active.is_some(),"running_for_current_source":state.active.as_ref().is_some_and(|a|a.source==source),
            "last_attempt":attempt,"last_success_at_ms":state.success.as_ref().filter(|(scope,_)|scope==&source).map(|(_,time)|time),
            "consecutive_failures":state.failures.as_ref().filter(|(scope,_)|scope==&source).map(|(_,count)|*count).unwrap_or(0),
            "schedule_state":match plan {None=>"starting",Some((_,_,None))=>"invalid_time",Some(_)=>"scheduled"},
            "next_attempt_at_ms":plan.and_then(|(_,_,time)|*time)})
    }

    fn plan(&self, cfg: &AppConfig, next: Option<i64>) {
        self.state.lock().unwrap().plan = Some((
            Source::from_config(cfg),
            cfg.members.daily_pull_at.clone(),
            next,
        ));
    }
}
impl Attempt {
    pub fn finish(mut self, result: &Result<Value, ServiceError>) {
        let (status, reason, count) = match result {
            Ok(value) => ("succeeded", None, value["members"].as_array().map(Vec::len)),
            Err(error) => (
                "failed",
                Some(match error {
                    ServiceError::Conflict(_) | ServiceError::StaleRevision(_) => "context_changed",
                    ServiceError::Upstream(_) => "receiver_error",
                    ServiceError::Internal(_) => "local_storage",
                    _ => "invalid_request",
                }),
                None,
            ),
        };
        self.monitor.complete(self.id, status, reason, count);
        self.finished = true;
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.finished {
            self.monitor
                .complete(self.id, "cancelled", Some("interrupted_check_cache"), None);
        }
    }
}

/// Calendar scheduling: choose the first local occurrence, skip nonexistent DST
/// times rather than substituting "now", and never silently repair invalid input.
pub(crate) fn next_at<Tz: TimeZone>(now: DateTime<Tz>, at: &str) -> Option<i64> {
    let (hour, minute) = at.split_once(':')?;
    if hour.len() != 2
        || minute.len() != 2
        || !hour
            .bytes()
            .chain(minute.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    let mut date = now.date_naive();
    for _ in 0..3 {
        let naive = date.and_hms_opt(hour, minute, 0)?;
        if let Some(next) = now.timezone().from_local_datetime(&naive).earliest() {
            if next.timestamp_millis() > now.timestamp_millis() {
                return Some(next.timestamp_millis());
            }
        }
        date = date.succ_opt()?;
    }
    None
}

#[derive(Default)]
struct Schedule {
    planned: Option<(Source, String, Option<i64>)>,
}
impl Schedule {
    fn tick<Tz: TimeZone>(&mut self, cfg: &AppConfig, now: DateTime<Tz>) -> bool {
        let source = Source::from_config(cfg);
        let at = &cfg.members.daily_pull_at;
        if self
            .planned
            .as_ref()
            .is_none_or(|(scope, time, _)| scope != &source || time != at)
        {
            self.planned = Some((source, at.clone(), next_at(now, at)));
            return false; // Startup/source changes never cause immediate catch-up pulls.
        }
        if self
            .next()
            .is_some_and(|next| next <= now.timestamp_millis())
        {
            self.planned = Some((source, at.clone(), next_at(now, at)));
            return true;
        }
        false
    }
    fn next(&self) -> Option<i64> {
        self.planned.as_ref().and_then(|(_, _, next)| *next)
    }
}

async fn tick(state: &ApiState, schedule: &mut Schedule, now: DateTime<Local>) {
    let cfg = state.cfg.get().await;
    let due = schedule.tick(&cfg, now);
    state.members_pull.plan(&cfg, schedule.next());
    if due {
        if let Err(error) = super::members::refresh_members_from_gateway(state, &cfg).await {
            tracing::warn!(error=%error,"scheduled member pull failed; no automatic retry before next schedule");
        }
    }
}

pub fn spawn(state: Arc<ApiState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut schedule = Schedule::default();
        loop {
            tick(&state, &mut schedule, Local::now()).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    })
}
