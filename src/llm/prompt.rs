use crate::bus::IncomingEvent;
use crate::settings::AppConfig;

pub const DEFAULT_PROMPT: &str =
    "你是排谷消息解析器。只抽取结构化信息，不计算价格、不排序、不生成回复。";

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
        None => out.push('无'),
    }

    out.push_str(
        "\n\n# 输出契约\n只输出 JSON：\
{\"intent\":\"claim|cancel|admin|unknown\",\
\"items\":[{\"item\":\"商品名\",\"variant\":\"变体名\",\"quantity\":1,\
\"claim_type\":\"split|single\",\"slot_policy\":\"normal|tail|fullbox|wholebox\"}],\
\"confidence\":0.0,\"ambiguous_parts\":[]}。\
非排谷消息 → intent=unknown；商品歧义 → 填入 ambiguous_parts。\
种类：整盒 → claim_type=single & slot_policy=wholebox；包盒 → split & fullbox；包尾 → split & tail。",
    );
    out
}

/// §U7 first-match 澄清 system prompt。
pub fn build_first_match_system_prompt() -> String {
    "你是排谷解析澄清器。用户只报了角色名/变体名，未报商品大类，且无法唯一匹配。\
请依据给出的商品列表，按目录顺序选择第一个含该角色名的**可拼团商品**（拼团类/特典类）。\
允许修正错别字/同音字。只输出 JSON：\
{\"matches\":[{\"name\":\"用户所报名\",\"item\":\"商品名\",\"variant\":\"变体名\"}]}。\
不要计算价格、不要生成回复。"
        .to_string()
}

/// §U7 first-match 澄清 user prompt（含可拼团商品目录）。
pub fn build_first_match_user_prompt(
    cfg: &AppConfig,
    message: &str,
    unresolved: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("# 可拼团商品目录（按目录顺序，first-match 取第一个命中）\n");
    for item in cfg.round.to_items() {
        if !crate::parser::alias_match::is_splittable(&item) {
            continue;
        }
        out.push_str(&format!(
            "- {} (id={}, kind={}, 别名: {})",
            item.name,
            item.item_id.0,
            item.kind.as_str(),
            item.aliases.join("/")
        ));
        if !item.variants.is_empty() {
            let variants: Vec<&str> = item.variants.iter().map(|v| v.name.as_str()).collect();
            out.push_str(&format!(" 变体: {}", variants.join("、")));
        }
        out.push('\n');
    }
    out.push_str("\n# 待澄清的角色名\n");
    out.push_str(&unresolved.join("、"));
    out.push_str(&format!("\n\n# 原始消息\n{message}"));
    out
}

pub fn build_user_prompt(ev: &IncomingEvent) -> String {
    format!(
        "群: {}\n用户ID: {}\n昵称: {}\n消息: {}",
        ev.group_id, ev.user_id, ev.nickname, ev.text
    )
}

/// 别名建议输入项（§U6 别名「LLM 建议」；一次请求覆盖全部商品）。
pub struct AliasPromptItem<'a> {
    pub item_id: &'a str,
    pub name: &'a str,
    pub aliases: &'a [String],
}

/// §U6 别名建议 system prompt（要求返回 JSON）。
pub fn build_alias_system_prompt() -> String {
    "你是排谷商品的别名整理助手。为每个商品补全/优化便于群友口语匹配的别名\
（中文简称、常见错别字或同音字、系列/作品相关称呼等），不要编造与原商品无关的别名，\
不要计算价格、不要生成回复。只输出 JSON：\
{\"suggestions\":[{\"item_id\":\"原样返回 item_id\",\"aliases\":[\"...\"],\"verdict\":\"filled\"}]}。\
suggestions 必须覆盖输入中的每个 item_id；若该商品现有别名已足够完善、无需改动，\
则 `aliases` 原样返回且 `verdict=\"best\"`；否则返回新别名集合且 `verdict=\"filled\"`。"
        .to_string()
}

/// §U6 别名建议 user prompt（列出 item_id/名称/现有别名）。
pub fn build_alias_user_prompt(items: &[AliasPromptItem<'_>]) -> String {
    let mut out = String::from("# 待整理商品（请一次性全部处理）\n");
    for it in items {
        out.push_str(&format!(
            "- item_id={} name={} 现有别名: {}",
            it.item_id,
            it.name,
            it.aliases.join("/")
        ));
        out.push('\n');
    }
    out.push_str("\n只输出 JSON，suggestions 覆盖上面每个 item_id。");
    out
}
