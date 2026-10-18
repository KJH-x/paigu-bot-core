//! A recoverable config/round commit. The private pending record is authoritative
//! until both files have been installed; in-memory publication happens afterwards.
use super::AppConfig;
use crate::persistence::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Pending {
    version: u8,
    checksum: String,
    config: AppConfig,
}
fn pending_path(path: &Path) -> PathBuf {
    path.with_extension("pending.json")
}

pub(super) fn has_pending(path: &Path) -> bool {
    pending_path(path).exists()
}

fn install(path: &Path, cfg: &AppConfig) -> anyhow::Result<()> {
    super::rounds::write_round(&cfg.round)?;
    atomic_write(path, &serde_json::to_vec_pretty(cfg)?)?;
    Ok(())
}

pub(super) fn recover(path: &Path) -> anyhow::Result<Option<AppConfig>> {
    let pending = pending_path(path);
    if !pending.exists() {
        return Ok(None);
    }
    let entry: Pending = serde_json::from_slice(&std::fs::read(&pending)?)?;
    anyhow::ensure!(
        entry.version == 1 && entry.checksum == crate::messages::transactions::hash(&entry.config)?,
        "配置事务版本或校验值损坏，未自动覆盖配置"
    );
    if let Ok(raw) = std::fs::read(path) {
        if let Ok(current) = serde_json::from_slice::<AppConfig>(&raw) {
            anyhow::ensure!(
                current.revision <= entry.config.revision,
                "配置已有更高版本，拒绝覆盖为旧待恢复事务"
            );
        }
    }
    install(path, &entry.config)?;
    std::fs::remove_file(pending)?;
    Ok(Some(entry.config))
}

pub(super) fn persist(path: &Path, cfg: &AppConfig) -> anyhow::Result<()> {
    anyhow::ensure!(
        !pending_path(path).exists(),
        "上笔配置事务尚未恢复，未创建新的提交"
    );
    let entry = Pending {
        version: 1,
        checksum: crate::messages::transactions::hash(cfg)?,
        config: cfg.clone(),
    };
    atomic_write(&pending_path(path), &serde_json::to_vec(&entry)?)?;
    install(path, cfg)
        .map_err(|error| anyhow::anyhow!("配置事务已准备但尚未完成，请重载恢复；{error}"))?;
    std::fs::remove_file(pending_path(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn prepared_config_recovers_at_each_install_boundary_and_replaces_existing_files() {
        for boundary in [0, 1, 2] {
            let dir =
                std::env::temp_dir().join(format!("paigu-config-commit-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("app.json");
            let mut old = super::super::default_config();
            old.round.round_id = format!("commit_{}", uuid::Uuid::new_v4().simple());
            old.active_round_id = Some(old.round.round_id.clone());
            old.revision = 7;
            persist(&path, &old).unwrap();
            let mut next = old.clone();
            next.revision = 8;
            next.round.title = "恢复后的占位轮次".into();
            let entry = Pending {
                version: 1,
                checksum: crate::messages::transactions::hash(&next).unwrap(),
                config: next.clone(),
            };
            atomic_write(&pending_path(&path), &serde_json::to_vec(&entry).unwrap()).unwrap();
            if boundary >= 1 {
                super::super::rounds::write_round(&next.round).unwrap();
            }
            if boundary >= 2 {
                atomic_write(&path, &serde_json::to_vec(&next).unwrap()).unwrap();
            }
            let loaded = super::super::ConfigStore::load(&path).unwrap().get().await;
            assert_eq!(loaded.revision, 8);
            assert_eq!(loaded.round.title, next.round.title);
            assert_eq!(
                super::super::rounds::read_round(&next.round.round_id)
                    .unwrap()
                    .title,
                next.round.title
            );
            assert!(!has_pending(&path));
        }
    }

    #[test]
    fn corrupted_pending_and_newer_destination_fail_closed() {
        for newer in [false, true] {
            let dir =
                std::env::temp_dir().join(format!("paigu-config-corrupt-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("app.json");
            let mut cfg = super::super::default_config();
            cfg.round.round_id = format!("commit_{}", uuid::Uuid::new_v4().simple());
            cfg.revision = 10;
            persist(&path, &cfg).unwrap();
            let before = std::fs::read(&path).unwrap();
            let mut pending_cfg = cfg.clone();
            pending_cfg.revision = if newer { 9 } else { 11 };
            let entry = Pending {
                version: 1,
                checksum: if newer {
                    crate::messages::transactions::hash(&pending_cfg).unwrap()
                } else {
                    "wrong".into()
                },
                config: pending_cfg,
            };
            atomic_write(&pending_path(&path), &serde_json::to_vec(&entry).unwrap()).unwrap();
            assert!(recover(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(has_pending(&path));
        }
    }
}
