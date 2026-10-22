use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::{value_to_str, AppConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberTarget {
    Known(String),
    Ambiguous(Vec<String>),
    Partial(Vec<String>),
    Unknown,
    Unavailable,
}

#[derive(Default)]
pub struct MemberTargets {
    ids: BTreeSet<String>,
    names: BTreeMap<String, BTreeSet<String>>,
    unavailable: bool,
}

pub fn target_token(value: &str) -> String {
    value.nfkc().collect::<String>().trim().to_string()
}

impl MemberTargets {
    pub fn load(cfg: &AppConfig, group_id: &str) -> Self {
        let mut index = Self::default();
        if group_id.trim().is_empty()
            || cfg.round.group_id != group_id
            || cfg.members.group_id != group_id
        {
            index.unavailable = true;
            return index;
        }
        let raw = match std::fs::read_to_string(&cfg.members.cache_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return index,
            Err(_) => {
                index.unavailable = true;
                return index;
            }
        };
        let result = (|| {
            let value: Value = serde_json::from_str(&raw).ok()?;
            if let Some(source) = value.get("group_id") {
                if value_to_str(source).as_deref() != Some(group_id) {
                    return None;
                }
            }
            let list = value.get("members").unwrap_or(&value).as_array()?;
            for entry in list {
                let uid = value_to_str(entry.get("user_id")?)?;
                if uid.trim().is_empty() || uid != uid.trim() || !index.ids.insert(uid.clone()) {
                    return None;
                }
                let card = entry.get("card").and_then(Value::as_str).unwrap_or("");
                let nickname = entry.get("nickname").and_then(Value::as_str).unwrap_or("");
                let raw = if card.trim().is_empty() {
                    nickname
                } else {
                    card
                };
                let (name, _) = crate::parser::normalize::clean_nickname(raw);
                index.insert(&name, &uid);
            }
            for entry in &cfg.members.cn_overrides {
                if index.ids.contains(&entry.user_id) {
                    index.insert(&entry.cn, &entry.user_id);
                    for alias in &entry.aliases {
                        index.insert(alias, &entry.user_id);
                    }
                }
            }
            Some(())
        })();
        if result.is_none() {
            return Self {
                unavailable: true,
                ..Self::default()
            };
        }
        index
    }

    fn insert(&mut self, name: &str, uid: &str) {
        let name = target_token(name);
        if !name.is_empty() {
            self.names.entry(name).or_default().insert(uid.into());
        }
    }

    pub fn resolve(&self, token: &str) -> MemberTarget {
        if self.unavailable {
            return MemberTarget::Unavailable;
        }
        let token = target_token(token);
        if token.is_empty() {
            return MemberTarget::Unknown;
        }
        if self.ids.contains(&token) {
            return MemberTarget::Known(token);
        }
        if let Some(ids) = self.names.get(&token) {
            return if ids.len() == 1 {
                MemberTarget::Known(ids.first().cloned().unwrap_or_default())
            } else {
                MemberTarget::Ambiguous(ids.iter().cloned().collect())
            };
        }
        let partial: BTreeSet<String> = self
            .names
            .iter()
            .filter(|(name, _)| name.contains(&token) || token.contains(name.as_str()))
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect();
        if partial.is_empty() {
            MemberTarget::Unknown
        } else {
            MemberTarget::Partial(partial.into_iter().collect())
        }
    }
}
