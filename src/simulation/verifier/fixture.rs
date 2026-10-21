use super::RoundFixture;
use crate::domain::ids::{ItemId, RoundId};
use crate::domain::item::{Item, ItemKind, ItemSubitem};
use crate::domain::money::MoneyCents;
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
        let kind = match kind_str {
            "single" => ItemKind::Single,
            "gift" => ItemKind::Gift,
            "shipping" => ItemKind::Shipping,
            "adjustment" => ItemKind::Adjustment,
            _ => ItemKind::Split,
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

    Ok(RoundFixture {
        round_id,
        title,
        group_id,
        items,
        priority_users,
        priority_window,
    })
}
