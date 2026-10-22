//! 临近同发送者消息合并（实验，2026-10-13）。
//!
//! 纯函数：把 **按 `(timestamp_ms, seq)` 排序、且已过滤 `routed=="message"`** 的候选，
//! 分组成"同发送者、相邻、同意图"的理解单元。实时（`Pipeline::ingest`）与重放
//! （`replay::session::compute`）共用本模块，保证 实时 == 重放。

use crate::domain::item::Item;
use crate::parser::parsed_event::ParsedIntent;
use crate::parser::rule_parser::RuleParser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupIntent {
    Claim,
    Cancel,
    Modify,
}

/// 单条候选（调用方负责排序与过滤）。
pub struct Candidate<'a> {
    pub user_id: &'a str,
    pub text: &'a str,
    pub is_admin: bool,
    /// 强制边界（如 `routed != "message"` 的 drop 记录）——单例且断开当前串。
    pub force_boundary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// 边界：管理员、`/` 命令、空文本 —— 单例且断开当前串。
    Boundary,
    /// 可合并意图 —— 同人同意图则并入。
    Join(GroupIntent),
    /// 无明确意图（闲聊/模糊）—— 保持 `Ignored`，单例且断开。
    Other,
}

fn contains_proxy_marker(text: &str) -> bool {
    text.contains("代排") || text.contains("(代)") || text.contains("（代）")
}

/// 匿名代排：文本含 `代排` / `(代)` / `（代）`（具名 `代<名>排:` 由 `proxy_segments` 先行处理）。
pub fn is_anonymous_proxy(text: &str) -> bool {
    contains_proxy_marker(text)
}

/// 分类：先取规则解析意图；仅 `Unknown` 时才用关键词/代排标记升级为 Claim。
pub fn classify(c: &Candidate<'_>, items: &[Item]) -> Classification {
    let t = c.text.trim();
    if c.force_boundary || c.is_admin || t.starts_with(['/', '／']) || t.is_empty() {
        return Classification::Boundary;
    }
    match RuleParser::parse(t, items, c.is_admin).intent {
        ParsedIntent::Claim => Classification::Join(GroupIntent::Claim),
        ParsedIntent::Modify => Classification::Join(GroupIntent::Modify),
        ParsedIntent::Cancel => Classification::Join(GroupIntent::Cancel),
        ParsedIntent::Unknown => {
            if t.contains('撤') || t.contains("取消") || t.contains('退') {
                Classification::Join(GroupIntent::Cancel)
            } else if t.contains('改') {
                Classification::Join(GroupIntent::Modify)
            } else if contains_proxy_marker(t) || t.contains('排') || t.contains("claim") {
                Classification::Join(GroupIntent::Claim)
            } else {
                Classification::Other
            }
        }
        _ => Classification::Other,
    }
}

/// 分组：每组是候选下标的有序列表（长度 1 = 独立）。
pub fn group_candidates(cands: &[Candidate<'_>], items: &[Item]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut key: Option<(String, GroupIntent)> = None;

    fn flush(cur: &mut Vec<usize>, groups: &mut Vec<Vec<usize>>) {
        if !cur.is_empty() {
            groups.push(std::mem::take(cur));
        }
    }

    for (i, c) in cands.iter().enumerate() {
        match classify(c, items) {
            Classification::Join(kind) => {
                let same = key
                    .as_ref()
                    .is_some_and(|(u, k)| u == c.user_id && *k == kind);
                if same {
                    cur.push(i);
                } else {
                    flush(&mut cur, &mut groups);
                    key = Some((c.user_id.to_string(), kind));
                    cur.push(i);
                }
            }
            Classification::Boundary | Classification::Other => {
                flush(&mut cur, &mut groups);
                key = None;
                groups.push(vec![i]);
            }
        }
    }
    flush(&mut cur, &mut groups);
    groups
}

/// 组文本：成员按时间序 `\n` 连接。
pub fn joined_text(cands: &[Candidate<'_>], group: &[usize]) -> String {
    group
        .iter()
        .map(|&i| cands[i].text.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
