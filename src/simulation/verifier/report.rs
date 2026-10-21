use super::VerifyResult;
use std::path::Path;

pub(super) fn write_outputs(out_dir: &Path, result: &VerifyResult) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir)?;

    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(out_dir.join("result.json"), json)?;

    let mut jsonl = String::new();
    for o in &result.outcomes {
        jsonl.push_str(&serde_json::to_string(o)?);
        jsonl.push('\n');
    }
    std::fs::write(out_dir.join("outcomes.jsonl"), jsonl)?;

    let report = render_report(result);
    std::fs::write(out_dir.join("report.md"), report)?;

    Ok(())
}

fn render_report(result: &VerifyResult) -> String {
    let mut s = String::new();
    s.push_str(&format!("# 模拟验证报告: {}\n\n", result.round_id));
    s.push_str(&format!("- group_id: {}\n", result.group_id));
    s.push_str(&format!("- 消息总数: {}\n", result.message_count));
    s.push_str(&format!("- 生效(Applied): {}\n", result.applied_count));
    s.push_str(&format!("- 拒绝(Rejected): {}\n", result.rejected_count));
    s.push_str(&format!(
        "- 需确认(NeedConfirm): {}\n",
        result.need_confirm_count
    ));
    s.push_str(&format!("- 忽略(Ignored): {}\n", result.ignored_count));
    s.push_str(&format!(
        "- 未支持(Unsupported): {}\n",
        result.unsupported_count
    ));
    s.push_str(&format!(
        "- 重复(Duplicate): {}\n\n",
        result.duplicate_count
    ));

    s.push_str("## 购物金优先权 (Eligibility)\n\n");
    if result.eligibility.is_empty() {
        s.push_str("(无)\n\n");
    } else {
        for e in &result.eligibility {
            s.push_str(&format!(
                "- user={} level={} note={} valid_from={}\n",
                e.user_id.0,
                e.priority_level,
                e.note.clone().unwrap_or_default(),
                e.valid_from.map(|t| t.to_rfc3339()).unwrap_or_default()
            ));
        }
        s.push('\n');
    }

    s.push_str("## 逐条消息结果\n\n");
    s.push_str("| seq | msg | user | status | text | detail |\n");
    s.push_str("|---|---|---|---|---|---|\n");
    for o in &result.outcomes {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            o.source_sequence,
            o.message_id,
            o.user_id,
            o.status,
            o.text.replace('|', "\\|"),
            o.detail.replace('|', "\\|")
        ));
    }
    s.push('\n');

    s.push_str("## 最终排结果\n\n");
    for ia in &result.final_snapshot.item_allocations {
        let subitem_label = ia
            .subitem_id
            .as_ref()
            .map(|v| format!(" subitem={}", v))
            .unwrap_or_default();
        s.push_str(&format!(
            "### {} ({}, {}){}\n\n",
            ia.item_name, ia.item_id.0, ia.kind, subitem_label
        ));
        let mut boxes = ia.boxes.clone();
        boxes.sort_by_key(|b| b.box_index);
        for b in &boxes {
            let mut slots = b.slots.clone();
            slots.sort_by_key(|s| s.slot_index);
            let cells: Vec<String> = slots
                .iter()
                .map(|s| match s.status {
                    crate::domain::allocation::SlotStatus::Filled => s
                        .user_id
                        .as_ref()
                        .map(|u| u.0.clone())
                        .unwrap_or_else(|| "?".to_string()),
                    crate::domain::allocation::SlotStatus::LockedEmpty => "LOCKED".to_string(),
                    crate::domain::allocation::SlotStatus::AdminReserved => "ADMIN".to_string(),
                    crate::domain::allocation::SlotStatus::Empty => "·".to_string(),
                })
                .collect();
            s.push_str(&format!("- box{}: {}\n", b.box_index, cells.join(" | ")));
        }
        if !ia.singles.is_empty() {
            s.push_str("- 单领: ");
            let singles: Vec<String> = ia
                .singles
                .iter()
                .map(|x| format!("{}x{}", x.user_id.0, x.quantity))
                .collect();
            s.push_str(&singles.join(", "));
            s.push('\n');
        }
        if !ia.waiting.is_empty() {
            s.push_str("- 等待(waiting): ");
            let w: Vec<String> = ia
                .waiting
                .iter()
                .map(|x| format!("{}x{}", x.user_id.0, x.quantity))
                .collect();
            s.push_str(&w.join(", "));
            s.push('\n');
        }
        s.push('\n');
    }

    if let Some(sett) = &result.final_settlement {
        s.push_str("## 结算\n\n");
        for b in &sett.user_bills {
            s.push_str(&format!(
                "- {}: gross={} discount={} gift={} shipping={} final={}\n",
                b.user_id.0,
                b.gross_total.format_yuan(),
                b.discount_share.format_yuan(),
                b.gift_value_share.format_yuan(),
                b.shipping_fee.format_yuan(),
                b.final_total.format_yuan()
            ));
        }
    }

    s
}
