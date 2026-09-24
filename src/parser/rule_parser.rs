use crate::domain::item::{Item, ItemKind, ItemVariant};
use crate::parser::parsed_event::{ParsedClaimItem, ParsedIntent, ParsedMessage};

pub struct RuleParser;

struct Mention {
    item_idx: usize,
    variant_id: Option<String>,
    start: usize,
    end: usize,
}

struct Segment<'a> {
    item_idx: usize,
    variant_id: Option<String>,
    text: &'a str,
}

fn same_target(a: &Mention, b: &Mention) -> bool {
    a.item_idx == b.item_idx && a.variant_id == b.variant_id
}

impl RuleParser {
    pub fn parse(raw: &str, items: &[Item], _is_admin: bool) -> ParsedMessage {
        let text = normalize_text(raw);
        let trimmed = text.trim();

        if trimmed.is_empty() {
            return ParsedMessage {
                intent: ParsedIntent::Unknown,
                round_hint: None,
                items: vec![],
                cancel_target_hint: None,
                admin_command: None,
                confidence: 0.1,
                ambiguous_parts: vec!["空消息".to_string()],
            };
        }

        let mentions = find_mentions(trimmed, items);
        let has_cancel = contains_any(trimmed, &["撤", "取消", "退了", "不要了", "退"]);
        let has_modify = contains_any(trimmed, &["改", "改成", "改为"]);

        if mentions.is_empty() {
            if has_cancel {
                return ParsedMessage {
                    intent: ParsedIntent::Cancel,
                    round_hint: None,
                    items: vec![],
                    cancel_target_hint: Some(trimmed.to_string()),
                    admin_command: None,
                    confidence: 0.9,
                    ambiguous_parts: vec![],
                };
            }
            if has_modify {
                return ParsedMessage {
                    intent: ParsedIntent::Modify,
                    round_hint: None,
                    items: vec![],
                    cancel_target_hint: Some(trimmed.to_string()),
                    admin_command: None,
                    confidence: 0.6,
                    ambiguous_parts: vec![],
                };
            }
            return ParsedMessage {
                intent: ParsedIntent::Unknown,
                round_hint: None,
                items: vec![],
                cancel_target_hint: None,
                admin_command: None,
                confidence: 0.2,
                ambiguous_parts: vec![],
            };
        }

        let segments = build_segments(trimmed, &mentions);
        let mut parsed_items: Vec<ParsedClaimItem> = Vec::new();

        for seg in &segments {
            let item = &items[seg.item_idx];
            let variant = seg
                .variant_id
                .as_ref()
                .and_then(|vid| item.find_variant_by_id(vid));
            let policy = detect_policy(seg.text);
            let quantity = parse_quantity(seg.text, item, variant, &policy);
            let claim_type = detect_claim_type(seg.text, item, &policy);
            let is_proxy = seg.text.contains("代牌");

            parsed_items.push(ParsedClaimItem {
                name: variant
                    .map(|v| v.name.clone())
                    .unwrap_or_else(|| item.name.clone()),
                category_hint: None,
                quantity,
                claim_type: Some(claim_type),
                is_proxy_card: Some(is_proxy),
                slot_policy: Some(policy),
                notes: None,
                resolved_item_id: Some(item.item_id.0.clone()),
                resolved_variant_id: seg.variant_id.clone(),
                resolved_round_id: Some(item.round_id.0.clone()),
            });
        }

        let mut ambiguous = Vec::new();
        let has_explicit_claim_verb = trimmed.contains('排') || trimmed.contains("claim");
        if has_cancel && has_explicit_claim_verb {
            ambiguous.push("同一消息同时包含排谷与撤销意图".to_string());
        }

        let intent = if has_cancel {
            ParsedIntent::Cancel
        } else if has_modify {
            ParsedIntent::Modify
        } else {
            ParsedIntent::Claim
        };

        let is_cancel_intent = intent == ParsedIntent::Cancel;

        ParsedMessage {
            intent,
            round_hint: None,
            items: parsed_items,
            cancel_target_hint: if is_cancel_intent {
                Some(trimmed.to_string())
            } else {
                None
            },
            admin_command: None,
            confidence: if ambiguous.is_empty() { 0.95 } else { 0.4 },
            ambiguous_parts: ambiguous,
        }
    }
}

fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let c = match ch {
            '０'..='９' => char::from_u32(ch as u32 - 0xFF10 + '0' as u32).unwrap_or(ch),
            'Ａ'..='Ｚ' => char::from_u32(ch as u32 - 0xFF21 + 'A' as u32).unwrap_or(ch),
            'ａ'..='ｚ' => char::from_u32(ch as u32 - 0xFF41 + 'a' as u32).unwrap_or(ch),
            '＋' => '+',
            '，' => ',',
            '：' => ':',
            '（' => '(',
            '）' => ')',
            '　' => ' ',
            _ => ch,
        };
        out.push(c);
    }
    out
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

fn collect_token_mentions(
    all: &mut Vec<Mention>,
    text: &str,
    item_idx: usize,
    variant_id: Option<String>,
    tok: &str,
) {
    if tok.is_empty() {
        return;
    }
    let mut from = 0usize;
    while from <= text.len() {
        match text[from..].find(tok) {
            Some(pos) => {
                let abs = from + pos;
                all.push(Mention {
                    item_idx,
                    variant_id: variant_id.clone(),
                    start: abs,
                    end: abs + tok.len(),
                });
                from = abs + tok.len();
                if from >= text.len() {
                    break;
                }
            }
            None => break,
        }
    }
}

fn find_mentions(text: &str, items: &[Item]) -> Vec<Mention> {
    let mut item_mentions: Vec<Mention> = Vec::new();
    let mut variant_mentions: Vec<(Mention, String)> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        let mut tokens: Vec<String> = vec![normalize_text(&item.name)];
        tokens.extend(item.aliases.iter().map(|a| normalize_text(a)));
        for tok in tokens {
            collect_token_mentions(&mut item_mentions, text, idx, None, &tok);
        }
        for variant in &item.variants {
            let vname = normalize_text(&variant.name);
            let mut vtoks: Vec<String> = vec![vname.clone()];
            vtoks.extend(variant.aliases.iter().map(|a| normalize_text(a)));
            for tok in vtoks {
                let mut tmp: Vec<Mention> = Vec::new();
                collect_token_mentions(&mut tmp, text, idx, Some(variant.variant_id.clone()), &tok);
                for m in tmp {
                    variant_mentions.push((m, vname.clone()));
                }
            }
        }
    }

    // Context resolution: bind each variant mention to the nearest item mention,
    // so that e.g. "结城理通行证" resolves to 通行证's 结城理 (not another item's).
    for (vm, vname) in variant_mentions.iter_mut() {
        if item_mentions.is_empty() {
            continue;
        }
        // Prefer an item name adjacent to the variant, since it usually qualifies it
        // ("结城理通行证" / "通行证结城理" / "特典 虎狼丸"); fall back to nearest by center.
        let mut chosen: Option<usize> = None;
        let mut best_gap = usize::MAX;
        for im in &item_mentions {
            if im.start >= vm.end {
                let gap = im.start - vm.end;
                if gap <= 6 && gap < best_gap {
                    best_gap = gap;
                    chosen = Some(im.item_idx);
                }
            }
        }
        if chosen.is_none() {
            for im in &item_mentions {
                if im.end <= vm.start {
                    let gap = vm.start - im.end;
                    if gap <= 6 && gap < best_gap {
                        best_gap = gap;
                        chosen = Some(im.item_idx);
                    }
                }
            }
        }
        if chosen.is_none() {
            let vc = (vm.start + vm.end) / 2;
            let mut best_d: Option<usize> = None;
            for im in &item_mentions {
                let ic = (im.start + im.end) / 2;
                let d = ic.abs_diff(vc);
                if best_d.is_none() || d < best_d.unwrap() {
                    best_d = Some(d);
                    chosen = Some(im.item_idx);
                }
            }
        }
        if let Some(ti) = chosen {
            if let Some(tv) = items[ti]
                .variants
                .iter()
                .find(|v| normalize_text(&v.name) == *vname)
            {
                vm.item_idx = ti;
                vm.variant_id = Some(tv.variant_id.clone());
            }
        }
    }

    // Drop an item mention that merely qualifies an adjacent same-item variant
    // (e.g. "特典卡组-校园凭证 虎狼丸" -> only the variant is claimed, not the base item).
    let is_sep_gap = |a: &Mention, b: &Mention| -> bool {
        let (s, e) = if a.end <= b.start {
            (a.end, b.start)
        } else if b.end <= a.start {
            (b.end, a.start)
        } else {
            return true;
        };
        let gap = &text[s..e];
        // The gap may still contain the tail of the item name (e.g. "-校园凭证 ");
        // as long as it holds no digit, treat it as a qualifier gap.
        gap.len() <= 24 && !gap.chars().any(|c| c.is_ascii_digit())
    };
    item_mentions.retain(|im| {
        !variant_mentions
            .iter()
            .any(|(vm, _)| vm.item_idx == im.item_idx && is_sep_gap(im, vm))
    });

    if std::env::var("PAIGU_DEBUG_MENTIONS").is_ok() {
        for (i, m) in item_mentions.iter().enumerate() {
            eprintln!("ITEM[{}] item_idx={} {}..{}", i, m.item_idx, m.start, m.end);
        }
        for (m, n) in variant_mentions.iter() {
            eprintln!("VAR {} item_idx={} {}..{}", n, m.item_idx, m.start, m.end);
        }
    }

    let mut all = item_mentions;
    all.extend(variant_mentions.into_iter().map(|(m, _)| m));

    all.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

    let mut merged: Vec<Mention> = Vec::new();
    for m in all {
        let mut handled = false;
        if let Some(last) = merged.last_mut() {
            if m.start < last.end {
                if same_target(&m, last) {
                    if m.end > last.end {
                        last.end = m.end;
                    }
                } else if (m.end - m.start) > (last.end - last.start) {
                    *last = Mention {
                        item_idx: m.item_idx,
                        variant_id: m.variant_id.clone(),
                        start: m.start,
                        end: m.end,
                    };
                }
                handled = true;
            } else if same_target(&m, last) && m.start == last.end {
                last.end = m.end;
                handled = true;
            }
        }
        if !handled {
            merged.push(m);
        }
    }

    let mut result: Vec<Mention> = Vec::new();
    for m in merged {
        if let Some(last) = result.last_mut() {
            if same_target(last, &m) && m.start >= last.end {
                let gap = &text[last.end..m.start];
                if !gap.chars().any(|c| c.is_ascii_digit()) {
                    last.end = m.end;
                    continue;
                }
            }
        }
        result.push(m);
    }

    result
}

fn build_segments<'a>(text: &'a str, mentions: &[Mention]) -> Vec<Segment<'a>> {
    let mut segs = Vec::new();
    let n = mentions.len();
    for i in 0..n {
        let start = if i == 0 { 0 } else { mentions[i].start };
        let end = if i + 1 < n {
            mentions[i + 1].start
        } else {
            text.len()
        };
        if start <= end && end <= text.len() {
            segs.push(Segment {
                item_idx: mentions[i].item_idx,
                variant_id: mentions[i].variant_id.clone(),
                text: &text[start..end],
            });
        }
    }
    segs
}

fn detect_policy(text: &str) -> String {
    // §U6/U8：`整盒` 是独立种类（单领队列），与 `包盒`(fullbox 拼团策略) 区分。
    if contains_any(text, &["整盒", "一整盒", "要整盒"]) && !text.contains('包') {
        return "WholeBox".to_string();
    }
    if contains_any(
        text,
        &["包盒", "包一盒", "一盒全包", "全包", "包整盒", "整一盒"],
    ) {
        return "FullBox".to_string();
    }
    if contains_any(
        text,
        &["包尾", "尾巴", "包个尾", "要尾", "留尾", "端盒", "端了"],
    ) {
        return "TailLocked".to_string();
    }
    if contains_any(text, &["锁列", "锁一整列", "锁一列"]) {
        return "ColumnLocked".to_string();
    }
    if text.trim_end().ends_with('尾') {
        return "TailLocked".to_string();
    }
    "Normal".to_string()
}

fn detect_claim_type(text: &str, item: &Item, policy: &str) -> String {
    if policy == "WholeBox" || text.contains("单领") {
        return "Single".to_string();
    }
    match item.kind {
        ItemKind::Single => "Single".to_string(),
        ItemKind::Gift => "GiftClaim".to_string(),
        _ => "Split".to_string(),
    }
}

fn parse_quantity(text: &str, item: &Item, variant: Option<&ItemVariant>, policy: &str) -> u32 {
    let cleaned = strip_item_tokens(text, item, variant);

    if let Some(n) = find_plus_number(&cleaned) {
        return n;
    }
    if let Some(n) = find_arabic_number(&cleaned) {
        return n;
    }
    if policy == "FullBox" {
        return variant
            .and_then(|v| v.capacity)
            .or(item.box_size)
            .unwrap_or(1);
    }
    if let Some(n) = find_chinese_number(&cleaned) {
        return n;
    }
    1
}

fn strip_item_tokens(text: &str, item: &Item, variant: Option<&ItemVariant>) -> String {
    let mut tokens: Vec<String> = vec![normalize_text(&item.name)];
    tokens.extend(item.aliases.iter().map(|a| normalize_text(a)));
    if let Some(v) = variant {
        tokens.push(normalize_text(&v.name));
    }
    tokens.retain(|t| !t.is_empty());
    tokens.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let mut out = text.to_string();
    for t in tokens {
        out = out.replace(&t, " ");
    }
    out
}

fn find_plus_number(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '+' {
            let mut j = i + 1;
            let mut num = String::new();
            while j < chars.len() && chars[j].is_ascii_digit() {
                num.push(chars[j]);
                j += 1;
            }
            if !num.is_empty() {
                if let Ok(v) = num.parse::<u32>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn find_arabic_number(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut num = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                num.push(chars[i]);
                i += 1;
            }
            if let Ok(v) = num.parse::<u32>() {
                return Some(v);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn cn_digit(c: char) -> Option<u32> {
    match c {
        '零' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn is_cn_numeral(c: char) -> bool {
    cn_digit(c).is_some() || c == '十' || c == '百' || c == '千'
}

fn find_chinese_number(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_cn_numeral(chars[i]) {
            let start = i;
            while i < chars.len() && is_cn_numeral(chars[i]) {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            if let Some(n) = eval_cn(&run) {
                return Some(n);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn eval_cn(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut section: u32 = 0;
    let mut current: Option<u32> = None;
    for c in s.chars() {
        if let Some(d) = cn_digit(c) {
            current = Some(d);
        } else if c == '十' {
            section += current.take().unwrap_or(1) * 10;
        } else if c == '百' {
            section += current.take().unwrap_or(1) * 100;
        } else if c == '千' {
            section += current.take().unwrap_or(1) * 1000;
        } else {
            return None;
        }
    }
    if let Some(d) = current {
        section += d;
    }
    Some(section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ids::{ItemId, RoundId};
    use crate::domain::money::MoneyCents;

    fn split_item() -> Item {
        Item {
            item_id: ItemId("pass_sp".to_string()),
            round_id: RoundId("r1".to_string()),
            name: "通行认证SP-月行水上".to_string(),
            kind: ItemKind::Split,
            unit_price: MoneyCents(0),
            box_size: Some(8),
            max_quantity: None,
            is_blind: false,
            is_proxy_card: false,
            aliases: vec!["通行证".to_string()],
            sort_order: 0,
            metadata: serde_json::Value::Null,
            variants: vec![ItemVariant {
                variant_id: "v_jcl".to_string(),
                name: "结城理".to_string(),
                unit_price: MoneyCents(0),
                capacity: None,
                aliases: vec![],
            }],
        }
    }

    #[test]
    fn detect_policy_distinguishes_whole_box_from_full_box() {
        assert_eq!(detect_policy("整盒"), "WholeBox");
        assert_eq!(detect_policy("燐音吧唧整盒"), "WholeBox");
        assert_eq!(detect_policy("包盒"), "FullBox");
        assert_eq!(detect_policy("包整盒"), "FullBox");
        assert_eq!(detect_policy("包尾"), "TailLocked");
    }

    #[test]
    fn whole_box_parses_as_single_claim() {
        let msg = RuleParser::parse("排 通行证 结城理 整盒", &[split_item()], false);
        assert_eq!(msg.intent, ParsedIntent::Claim);
        assert_eq!(msg.items.len(), 1);
        assert_eq!(msg.items[0].claim_type.as_deref(), Some("Single"));
        assert_eq!(msg.items[0].slot_policy.as_deref(), Some("WholeBox"));
    }

    #[test]
    fn full_box_parses_as_split_fullbox() {
        let msg = RuleParser::parse("排 通行证 结城理 包盒", &[split_item()], false);
        assert_eq!(msg.items[0].claim_type.as_deref(), Some("Split"));
        assert_eq!(msg.items[0].slot_policy.as_deref(), Some("FullBox"));
    }

    #[test]
    fn tail_parses_as_split_tail_locked() {
        let msg = RuleParser::parse("排 通行证 结城理 包尾", &[split_item()], false);
        assert_eq!(msg.items[0].claim_type.as_deref(), Some("Split"));
        assert_eq!(msg.items[0].slot_policy.as_deref(), Some("TailLocked"));
    }
}
