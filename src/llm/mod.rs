pub mod client;
pub mod pipeline;
pub mod prompt;

pub use pipeline::Pipeline;

/// 从 LLM 原始输出中提取 JSON 主体（去 Markdown 代码围栏，截取首尾大括号之间）。
pub(crate) fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match (stripped.find('{'), stripped.rfind('}')) {
        (Some(start), Some(end)) if end > start => stripped[start..=end].to_string(),
        _ => stripped.to_string(),
    }
}

/// 按字符边界截断文本并追加省略号（日志/错误信息去重公共实现）。
pub(crate) fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
