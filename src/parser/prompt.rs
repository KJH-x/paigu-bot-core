pub fn build_system_prompt() -> String {
    crate::parser::parsed_event::SYSTEM_PROMPT.to_string()
}
