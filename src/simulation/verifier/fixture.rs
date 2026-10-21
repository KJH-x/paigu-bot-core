use super::RoundFixture;
use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::{Item, ItemKind, ItemSubitem};
use crate::domain::money::MoneyCents;
use crate::settings::{default_config, ItemCategory, ItemConfig, PriorityWindow, SubitemConfig};
use serde_json::Value;
use std::path::Path;

pub(crate) fn load_round_fixture(path: &Path) -> anyhow::Result<RoundFixture> {
    let raw = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;

    let round_id = v
        .get("round_id")
        .and_then(|x| x.as_str())
        .unwrap_or("round")
        .to_string();
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let group_id = v
        .get("group_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let priority_users: Vec<String> = v
        .get("priority_users")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let priority_window = v.get("priority_window").and_then(|w| {
        let s = w.get("start_ms").and_then(|x| x.as_i64());
        let e = w.get("end_ms").and_then(|x| x.as_i64());
        match (s, e) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        }
    });

    let items_v = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for (idx, iv) in items_v.iter().enumerate() {
        let item_id = iv
            .get("item_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let name = iv
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let kind_str = iv.get("kind").and_then(|x| x.as_str()).unwrap_or("split");
        let kind = match ItemCategory::parse(kind_str) {
            Some(ItemCategory::Single) => ItemKind::Single,
            Some(ItemCategory::Box) => ItemKind::WholeBox,
            Some(ItemCategory::Gift) => ItemKind::Gift,
            _ => match kind_str {
                "shipping" => ItemKind::Shipping,
                "adjustment" => ItemKind::Adjustment,
                _ => ItemKind::Split,
            },
        };
        let unit_price_cents = iv
            .get("unit_price_cents")
            .and_then(|x| x.as_i64())
            .or_else(|| iv.get("unit_price").and_then(|x| x.as_i64()))
            .unwrap_or(0);
        let box_size = iv
            .get("box_size")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let max_quantity = iv
            .get("max_quantity")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let aliases: Vec<String> = iv
            .get("aliases")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let subitems: Vec<ItemSubitem> = iv
            .get("subitems")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|vv| {
                        let subitem_id = vv
                            .get("subitem_id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let subitem_name = vv
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let subitem_price = vv
                            .get("unit_price_cents")
                            .and_then(|x| x.as_i64())
                            .or_else(|| vv.get("unit_price").and_then(|x| x.as_i64()))
                            .unwrap_or(0);
                        let capacity = vv
                            .get("capacity")
                            .and_then(|x| x.as_u64())
                            .map(|x| x as u32);
                        let subitem_aliases: Vec<String> = vv
                            .get("aliases")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        ItemSubitem {
                            subitem_id,
                            name: subitem_name,
                            unit_price: MoneyCents(subitem_price),
                            capacity,
                            aliases: subitem_aliases,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        items.push(Item {
            item_id: ItemId(if item_id.is_empty() {
                format!("item_{}", idx)
            } else {
                item_id
            }),
            round_id: RoundId(round_id.clone()),
            name,
            kind,
            unit_price: MoneyCents(unit_price_cents),
            box_size,
            max_quantity,
            is_blind: false,
            is_proxy_card: false,
            aliases,
            sort_order: idx as i32,
            metadata: Value::Null,
            subitems,
        });
    }

    let mut config = default_config();
    config.active_round_id = None;
    config.llm.enabled = false;
    config.gateway.reply_enabled = false;
    config.gateway.allowed_actions.clear();
    config.gateway.admin_commands_enabled = false;
    config.gateway.accept_forged_messages = false;
    config.gateway.react_emoji_id = None;
    config.gateway.bind = "127.0.0.1:0".into();
    config.gateway.whitelist_groups = vec![group_id.clone()];
    config.gateway.whitelist_members = serde_json::from_value(
        v.get("whitelist_members")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )?;
    config.members.cache_path.clear();
    config.members.group_id = group_id.clone();
    config.members.cn_overrides = serde_json::from_value(
        v.get("members")
            .and_then(|m| m.get("cn_overrides"))
            .or_else(|| v.get("cn_overrides"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )?;
    config.round.round_id = round_id;
    config.round.title = title;
    config.round.group_id = group_id;
    config.round.priority_users = priority_users;
    config.round.priority_window =
        priority_window.map(|(start_ms, end_ms)| PriorityWindow { start_ms, end_ms });
    config.round.phases = serde_json::from_value(
        v.get("phases")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )?;
    config.round.items = catalog_settings(&items);
    for (index, item) in config.round.items.iter_mut().enumerate() {
        item.class = items_v[index]
            .get("class")
            .and_then(Value::as_str)
            .map(str::to_string);
        item.box_item_id = items_v[index]
            .get("box_item_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        for (position, subitem) in item.subitems.iter_mut().enumerate() {
            subitem.adjust_cents = items_v[index]
                .get("subitems")
                .and_then(|a| a.get(position))
                .and_then(|s| s.get("adjust_cents"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
        }
    }
    config.merge = serde_json::from_value(
        v.get("merge")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )?;
    let fixture_version: u8 = serde_json::from_value(
        v.get("fixture_version")
            .cloned()
            .unwrap_or_else(|| serde_json::json!(1)),
    )?;
    anyhow::ensure!(matches!(fixture_version, 1 | 2), "未知离线夹具版本");
    Ok(RoundFixture {
        fixture_version,
        config,
        items,
    })
}

pub(super) fn catalog_settings(items: &[Item]) -> Vec<ItemConfig> {
    items
        .iter()
        .map(|item| ItemConfig {
            item_id: item.item_id.0.clone(),
            name: item.name.clone(),
            kind: item.kind.as_str().to_string(),
            class: None,
            aliases: item.aliases.clone(),
            unit_price_cents: item.unit_price.0,
            max_quantity: item.max_quantity,
            box_item_id: None,
            subitems: item
                .subitems
                .iter()
                .map(|subitem| SubitemConfig {
                    subitem_id: subitem.subitem_id.clone(),
                    name: subitem.name.clone(),
                    unit_price_cents: subitem.unit_price.0,
                    adjust_cents: 0,
                    capacity: subitem.capacity,
                    aliases: subitem.aliases.clone(),
                })
                .collect(),
        })
        .collect()
}
