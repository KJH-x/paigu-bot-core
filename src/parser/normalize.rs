use crate::parser::parsed_event::ParsedClaimItem;

/// 全角字符归一为半角（`！`–`～` 段整体平移，全角空格 → 普通空格）。
pub fn normalize_fullwidth(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if (0xFF01..=0xFF5E).contains(&u) {
                char::from_u32(u - 0xFEE0).unwrap_or(c)
            } else if u == 0x3000 {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn split_proxy(s: &str) -> Option<(String, String)> {
    if !s.ends_with(')') {
        return None;
    }
    let open = s.rfind('(')?;
    let inner = &s[open + 1..s.len() - 1];
    let rest = inner.strip_prefix('代')?;
    let a = s[..open].trim();
    let b = rest.trim();
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some((a.to_string(), b.to_string()))
}

fn strip_remark(s: &str) -> String {
    match s.char_indices().find(|(_, c)| *c == '(' || *c == '（') {
        Some((i, _)) => s[..i].trim().to_string(),
        None => s.trim().to_string(),
    }
}

/// 昵称清洗（D-05 下沉至中性模块）：全角归一 + 去括号备注 + 代理写法 `A(代B)`。
/// 返回 `(identity, display)`；`identity` 用于白名单/优先级匹配，`display` 用于展示。
pub fn clean_nickname(raw: &str) -> (String, String) {
    let normalized = normalize_fullwidth(raw);
    let t = normalized.trim();
    if let Some((a, b)) = split_proxy(t) {
        return (a.clone(), format!("{a}({b})"));
    }
    let identity = strip_remark(t);
    (identity.clone(), identity)
}

pub fn normalize_claim_item(item: &ParsedClaimItem) -> ParsedClaimItem {
    let mut normalized = item.clone();

    // §U6/U8 种类：`整盒` 是独立种类（进单领队列，按盒计数），不是 `fullbox`(包盒) 拼团策略。
    let raw_policy = normalized.slot_policy.clone().unwrap_or_default();
    let is_whole_box = is_whole_box_token(&raw_policy);

    normalized.claim_type = if is_whole_box {
        Some("Single".to_string())
    } else {
        normalized.claim_type.map(|ct| {
            let ct_lower = ct.to_lowercase();
            if ct_lower.contains("single") || ct_lower.contains("单领") || ct_lower.contains("单")
            {
                "Single".to_string()
            } else if ct_lower.contains("gift")
                || ct_lower.contains("赠")
                || ct_lower.contains("特典")
            {
                "GiftClaim".to_string()
            } else {
                "Split".to_string()
            }
        })
    };

    normalized.slot_policy = normalized.slot_policy.map(|sp| {
        let sp_lower = sp.to_lowercase();
        if is_whole_box_token(&sp) {
            "Normal".to_string()
        } else if sp_lower.contains("fullbox")
            || sp_lower.contains("full_box")
            || sp_lower.contains("包盒")
            || sp_lower.contains("全包")
            || sp_lower.contains("包一盒")
            || sp_lower.contains("一盒全包")
            || sp_lower.contains("包整盒")
            || sp_lower.contains("整一盒")
        {
            "FullBox".to_string()
        } else if sp_lower.contains("tail")
            || sp_lower.contains("包尾")
            || sp_lower.contains("端盒")
        {
            "TailLocked".to_string()
        } else if sp_lower.contains("column") || sp_lower.contains("锁列") {
            "ColumnLocked".to_string()
        } else if sp_lower.contains("admin") || sp_lower.contains("管理") {
            "AdminFixed".to_string()
        } else {
            "Normal".to_string()
        }
    });

    normalized
}

/// `整盒` 标记（含 RuleParser 的 `WholeBox`）；排除 `包整盒`/`包盒` 等 `fullbox` 说法。
fn is_whole_box_token(token: &str) -> bool {
    let lower = token.to_lowercase();
    if lower.contains("full") || token.contains('包') {
        return false;
    }
    lower.contains("wholebox") || lower.contains("whole_box") || token.contains("整盒")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_nickname_removes_remark() {
        assert_eq!(clean_nickname("甲（备注甲）").1, "甲");
        assert_eq!(clean_nickname("乙.（备注乙）").1, "乙.");
        assert_eq!(clean_nickname("丙/丁（备注丙）").1, "丙/丁");
        assert_eq!(clean_nickname("戊（备注戊👀）").1, "戊");
    }

    #[test]
    fn clean_nickname_proxy_notation() {
        let (identity, display) = clean_nickname("A（代B）");
        assert_eq!(identity, "A");
        assert_eq!(display, "A(B)");
        assert_eq!(clean_nickname("甲(代 乙)").0, "甲");
    }

    #[test]
    fn clean_nickname_normalizes_fullwidth() {
        assert_eq!(clean_nickname("名：015").1, "名:015");
    }

    fn claim(slot_policy: &str, claim_type: Option<&str>) -> ParsedClaimItem {
        ParsedClaimItem {
            name: "商品".to_string(),
            category_hint: None,
            quantity: 1,
            claim_type: claim_type.map(str::to_string),
            is_proxy_card: None,
            slot_policy: Some(slot_policy.to_string()),
            notes: None,
            resolved_item_id: None,
            resolved_variant_id: None,
            resolved_round_id: None,
        }
    }

    #[test]
    fn whole_box_maps_to_single_queue() {
        let n = normalize_claim_item(&claim("整盒", Some("Split")));
        assert_eq!(n.claim_type.as_deref(), Some("Single"));
        assert_eq!(n.slot_policy.as_deref(), Some("Normal"));

        let n2 = normalize_claim_item(&claim("WholeBox", Some("Split")));
        assert_eq!(n2.claim_type.as_deref(), Some("Single"));
        assert_eq!(n2.slot_policy.as_deref(), Some("Normal"));
    }

    #[test]
    fn full_box_remains_split_strategy() {
        let n = normalize_claim_item(&claim("包盒", Some("Split")));
        assert_eq!(n.claim_type.as_deref(), Some("Split"));
        assert_eq!(n.slot_policy.as_deref(), Some("FullBox"));

        let n2 = normalize_claim_item(&claim("fullbox", Some("Split")));
        assert_eq!(n2.claim_type.as_deref(), Some("Split"));
        assert_eq!(n2.slot_policy.as_deref(), Some("FullBox"));

        let n3 = normalize_claim_item(&claim("包整盒", Some("Split")));
        assert_eq!(n3.claim_type.as_deref(), Some("Split"));
        assert_eq!(n3.slot_policy.as_deref(), Some("FullBox"));
    }

    #[test]
    fn tail_remains_split_tail_locked() {
        let n = normalize_claim_item(&claim("包尾", Some("Split")));
        assert_eq!(n.claim_type.as_deref(), Some("Split"));
        assert_eq!(n.slot_policy.as_deref(), Some("TailLocked"));
    }
}
