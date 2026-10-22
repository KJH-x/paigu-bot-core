use super::Gateway;
use crate::{
    bus::{Delivery, EventSink, IngressBinding, IngressSource},
    gateway::onebot,
    messages::MessageLog,
    settings::ConfigStore,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{mpsc, Mutex};

#[cfg(test)]
#[path = "intake_storage_tests.rs"]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Entry {
    version: u8,
    sequence: u64,
    pub(super) id: String,
    pub(super) binding: IngressBinding,
    config_revision: u64,
    pub(super) event: onebot::RouteMessageEvent,
    #[serde(default)]
    raw_event: Option<serde_json::Value>,
    #[serde(default)]
    raw_saved: bool,
    pub(super) drop_reason: Option<String>,
    emoji: Option<u32>,
}

#[derive(Default)]
struct State {
    initialized: bool,
    sequence: u64,
    counts: BTreeMap<String, usize>,
    error: Option<String>,
    paused: bool,
}

pub(super) struct Intake {
    directory: PathBuf,
    state: Mutex<State>,
    wake: std::sync::Mutex<mpsc::Sender<()>>,
    receiver: std::sync::Mutex<Option<mpsc::Receiver<()>>>,
}

impl Intake {
    pub(super) fn new(messages: &MessageLog) -> Arc<Self> {
        let (wake, receiver) = mpsc::channel(1);
        Arc::new(Self {
            directory: messages.ingress_dir(),
            state: Mutex::new(State::default()),
            wake: std::sync::Mutex::new(wake),
            receiver: std::sync::Mutex::new(Some(receiver)),
        })
    }
    pub(super) fn receiver(&self) -> Option<mpsc::Receiver<()>> {
        let mut receiver = self.receiver.lock().unwrap();
        if receiver.is_none() {
            // A failed worker owns no input; all work stays on disk. Recreate
            // only the coalesced wake channel when explicitly restarting it.
            let (wake, next) = mpsc::channel(1);
            *self.wake.lock().unwrap() = wake;
            *receiver = Some(next);
        }
        receiver.take()
    }
    pub(super) fn notify(&self) {
        let _ = self.wake.lock().unwrap().try_send(());
    }
    pub(super) async fn retry(&self) {
        let mut state = self.state.lock().await;
        state.paused = false;
        state.error = None;
        drop(state);
        self.notify();
    }
    fn path(&self, round: &str, seq: u64) -> PathBuf {
        self.directory.join(round).join(format!("{seq:020}.json"))
    }

    fn initialize(&self, state: &mut State) -> anyhow::Result<()> {
        if state.initialized {
            return Ok(());
        }
        let mut counts = BTreeMap::new();
        let mut maximum = 0;
        if self.directory.exists() {
            for round in std::fs::read_dir(&self.directory)? {
                let round = round?;
                anyhow::ensure!(
                    round.file_type()?.is_dir()
                        && crate::settings::rounds::is_valid_round_id(
                            &round.file_name().to_string_lossy()
                        ),
                    "待处理目录格式错误"
                );
                let id = round.file_name().to_string_lossy().to_string();
                for file in std::fs::read_dir(round.path())? {
                    let file = file?;
                    if file.path().extension().and_then(|e| e.to_str()) == Some("tmp") {
                        continue;
                    }
                    let sequence = sequence(&file.path())?;
                    anyhow::ensure!(file.file_type()?.is_file(), "待处理记录不是普通文件");
                    maximum = maximum.max(sequence);
                    *counts.entry(id.clone()).or_default() += 1;
                }
            }
        }
        state.sequence = maximum;
        state.counts = counts;
        state.initialized = true;
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn accept(
        &self,
        cfg: &crate::settings::AppConfig,
        event: onebot::RouteMessageEvent,
        drop_reason: Option<String>,
    ) -> anyhow::Result<()> {
        let raw = serde_json::to_value(&event)?;
        self.accept_raw(cfg, event, drop_reason, raw).await
    }

    pub(super) async fn accept_raw(
        &self,
        cfg: &crate::settings::AppConfig,
        event: onebot::RouteMessageEvent,
        drop_reason: Option<String>,
        raw: serde_json::Value,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            crate::settings::rounds::is_valid_round_id(&cfg.round.round_id),
            "入站轮次标识无效"
        );
        let mut state = self.state.lock().await;
        self.initialize(&mut state)?;
        let sequence = state
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("入站序号耗尽"))?;
        let entry = Entry {
            version: 1,
            sequence,
            id: uuid::Uuid::new_v4().to_string(),
            binding: IngressBinding::from_config(cfg),
            config_revision: cfg.revision,
            event,
            raw_event: Some(raw),
            raw_saved: false,
            drop_reason,
            emoji: cfg.gateway.react_emoji_id,
        };
        let path = self.path(&entry.binding.round_id, sequence);
        anyhow::ensure!(!path.exists(), "入站文件序号冲突");
        let raw = serde_json::to_vec(&entry)?;
        anyhow::ensure!(raw.len() <= 4 * 1024 * 1024, "入站记录过大");
        crate::persistence::atomic_write(&path, &raw)?;
        state.sequence = sequence;
        *state.counts.entry(entry.binding.round_id).or_default() += 1;
        drop(state);
        self.notify();
        Ok(())
    }

    async fn next(&self, round: &str) -> anyhow::Result<Option<Entry>> {
        let mut state = self.state.lock().await;
        self.initialize(&mut state)?;
        if state.counts.get(round).copied().unwrap_or(0) == 0 {
            return Ok(None);
        }
        let mut head = None;
        for file in std::fs::read_dir(self.directory.join(round))? {
            let file = file?;
            if file.path().extension().and_then(|e| e.to_str()) == Some("tmp") {
                continue;
            }
            let sequence = sequence(&file.path())?;
            anyhow::ensure!(file.file_type()?.is_file(), "待处理记录不是普通文件");
            if head.as_ref().is_none_or(|(seq, _)| sequence < *seq) {
                head = Some((sequence, file.path()));
            }
        }
        let (sequence, path) = head.ok_or_else(|| anyhow::anyhow!("待处理计数与文件不一致"))?;
        anyhow::ensure!(
            std::fs::metadata(&path)?.len() <= 4 * 1024 * 1024,
            "待处理记录过大"
        );
        let entry: Entry = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(
            entry.version == 1
                && entry.sequence == sequence
                && entry.binding.round_id == round
                && uuid::Uuid::parse_str(&entry.id).is_ok(),
            "待处理记录身份/版本损坏"
        );
        Ok(Some(entry))
    }

    async fn save(&self, entry: &Entry) -> anyhow::Result<()> {
        let _state = self.state.lock().await;
        crate::persistence::atomic_write(
            &self.path(&entry.binding.round_id, entry.sequence),
            &serde_json::to_vec(entry)?,
        )?;
        Ok(())
    }

    async fn ack(&self, entry: &Entry) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        std::fs::remove_file(self.path(&entry.binding.round_id, entry.sequence))?;
        if let Some(count) = state.counts.get_mut(&entry.binding.round_id) {
            *count = count.saturating_sub(1);
        }
        state.error = None;
        Ok(())
    }

    pub(super) async fn status(&self) -> serde_json::Value {
        let mut state = self.state.lock().await;
        if self.initialize(&mut state).is_err() {
            state.error = Some("待处理目录读取失败，未跳过或清空".into());
            state.paused = true;
        }
        serde_json::json!({"pending":state.counts.values().sum::<usize>(),"pending_by_round":state.counts,"wake_capacity":1,"last_error":state.error,"paused":state.paused})
    }
    pub(super) async fn fail(&self, message: &str) {
        let mut state = self.state.lock().await;
        if state.error.as_deref() != Some(message) {
            tracing::warn!("{message}");
        }
        state.error = Some(message.into());
        state.paused = true;
    }
}

fn sequence(path: &Path) -> anyhow::Result<u64> {
    anyhow::ensure!(
        path.extension().and_then(|e| e.to_str()) == Some("json"),
        "待处理区存在未知文件"
    );
    let stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("待处理文件名无效"))?;
    anyhow::ensure!(
        stem.len() == 20 && stem.bytes().all(|c| c.is_ascii_digit()),
        "待处理文件名无效"
    );
    Ok(stem.parse()?)
}

pub(super) async fn run(
    intake: Arc<Intake>,
    cfg: Arc<ConfigStore>,
    sink: Arc<dyn EventSink>,
    messages: Arc<MessageLog>,
    gateway: Weak<Gateway>,
    mut wake: mpsc::Receiver<()>,
) {
    loop {
        let config = cfg.get().await;
        let processed = async {
            if intake.state.lock().await.paused {
                return anyhow::Ok(false);
            }
            let Some(mut entry) = intake.next(&config.round.round_id).await? else {
                return anyhow::Ok(false);
            };
            if !entry.binding.matches(&config) {
                return Ok(false);
            }
            {
                let _writer = messages.write_guard().await;
                if !entry.binding.matches(&cfg.get().await) {
                    return Ok(false);
                }
                if !entry.raw_saved {
                    let raw = entry
                        .raw_event
                        .clone()
                        .unwrap_or(serde_json::to_value(&entry.event)?);
                    messages
                        .append_raw_event(&entry.binding.round_id, &raw)
                        .await?;
                    messages.sync_ingress_commit(&entry.binding.round_id)?;
                    entry.raw_saved = true;
                    intake.save(&entry).await?;
                }
            }
            let committed = if let Some(reason) = &entry.drop_reason {
                let _writer = messages.write_guard().await;
                if !entry.binding.matches(&cfg.get().await) {
                    false
                } else {
                    let source = IngressSource {
                        id: entry.id.clone(),
                        part: "drop".into(),
                    };
                    if !messages
                        .read_ingress_records(&entry.binding.round_id)
                        .await?
                        .iter()
                        .any(|r| r.ingress.as_ref() == Some(&source))
                    {
                        let mut record = onebot::to_message_record(
                            &entry.event,
                            &format!("drop:{reason}"),
                            "Dropped",
                            reason,
                        );
                        record.ingress = Some(source);
                        messages.append(&entry.binding.round_id, &record).await?;
                    }
                    true
                }
            } else {
                let mut event = onebot::to_incoming_event(&entry.event);
                event.raw = None; // Original frame is already durably retained above, including NC batches.
                event.ingress = Some(IngressSource {
                    id: entry.id.clone(),
                    part: "frame".into(),
                });
                sink.deliver(&entry.binding, event).await? == Delivery::Committed
            };
            if !committed {
                return Ok(false);
            }
            {
                let _writer = messages.write_guard().await;
                messages.sync_ingress_commit(&entry.binding.round_id)?;
                intake.ack(&entry).await?;
            }
            if entry.drop_reason.is_none() {
                if let (Some(emoji), Some(id), Some(gw)) =
                    (entry.emoji, entry.event.message_id, gateway.upgrade())
                {
                    if let Err(error) = gw
                        .dispatch(
                            "set_msg_emoji_like",
                            serde_json::json!({"message_id":id,"emoji_id":emoji}),
                        )
                        .await
                    {
                        tracing::warn!(error=%error,"failed to react with emoji");
                    }
                }
            }
            Ok(true)
        }
        .await;
        match processed {
            Ok(true) => continue,
            Ok(false) => {}
            Err(_) => {
                intake
                    .fail("入站待处理记录未完成；保留原文件等待恢复")
                    .await
            }
        }
        tokio::select! { item=wake.recv()=>{if item.is_none(){break;}},_=tokio::time::sleep(Duration::from_millis(250))=>{} }
    }
}
