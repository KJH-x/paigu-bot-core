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

    normalized.claim_type = normalized.claim_type.map(|ct| {
        let ct_lower = ct.to_lowercase();
        if ct_lower.contains("single") || ct_lower.contains("单领") || ct_lower.contains("单") {
            "Single".to_string()
        } else if ct_lower.contains("gift") || ct_lower.contains("赠") || ct_lower.contains("特典")
        {
            "GiftClaim".to_string()
        } else {
            "Split".to_string()
        }
    });

    normalized.slot_policy = normalized.slot_policy.map(|sp| {
        let sp_lower = sp.to_lowercase();
        if sp_lower.contains("fullbox")
            || sp_lower.contains("full_box")
            || sp_lower.contains("包盒")
            || sp_lower.contains("整盒")
            || sp_lower.contains("全包")
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
}
