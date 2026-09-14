pub mod client;
pub mod pipeline;
pub mod prompt;

pub use pipeline::Pipeline;

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
