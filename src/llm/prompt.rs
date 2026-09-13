use crate::bus::IncomingEvent;
use crate::settings::AppConfig;

pub const DEFAULT_PROMPT: &str = "你是排谷消息解析器。只抽取结构化信息，不计算价格、不排序、不生成回复。";

pub fn build_system_prompt(cfg: &AppConfig) -> String {
    let base = if cfg.llm.prompt_template.trim().is_empty() {
        DEFAULT_PROMPT
    } else {
        cfg.llm.prompt_template.as_str()
    };

    let mut out = String::new();
    out.push_str(base);
    out.push_str("\n\n# 商品目录\n");
    for item in &cfg.round.items {
        out.push_str(&format!(
            "- {} (id={}, kind={}, 别名: {})",
            item.name,
            item.item_id,
            item.kind,
            item.aliases.join("/")
        ));
        if !item.variants.is_empty() {
            let variants: Vec<&str> = item.variants.iter().map(|v| v.name.as_str()).collect();
            out.push_str(&format!(" 变体: {}", variants.join("、")));
        }
        out.push('\n');
    }

    out.push_str("\n# 预存(购物金)用户\n");
    out.push_str(&cfg.round.priority_users.join("、"));

    out.push_str("\n\n# 优先时段\n");
    match &cfg.round.priority_window {
        Some(window) => out.push_str(&format!(
            "[{}ms, {}ms) 内仅预存用户可排",
            window.start_ms, window.end_ms
        )),
        None => out.push_str("无"),
    }

    out.push_str(
        "\n\n# 输出契约\n只输出 JSON：\
{\"intent\":\"claim|cancel|admin|unknown\",\
\"items\":[{\"item\":\"商品名\",\"variant\":\"变体名\",\"quantity\":1,\
\"claim_type\":\"split|single\",\"slot_policy\":\"normal|tail|fullbox\"}],\
\"confidence\":0.0,\"ambiguous_parts\":[]}。\
非排谷消息 → intent=unknown；商品歧义 → 填入 ambiguous_parts。",
    );
    out
}

pub fn build_user_prompt(ev: &IncomingEvent) -> String {
    format!(
        "群: {}\n用户ID: {}\n昵称: {}\n消息: {}",
        ev.group_id, ev.user_id, ev.nickname, ev.text
    )
}
