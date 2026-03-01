pub fn default_system_prompt() -> String {
    include_str!("prompts/build_system_prompt.md").to_string()
}

pub fn build_system_prompt() -> String {
    include_str!("prompts/build_system_prompt.md").to_string()
}

pub fn plan_system_prompt() -> String {
    include_str!("prompts/plan_system_prompt.md").to_string()
}

pub fn explorer_system_prompt() -> String {
    include_str!("prompts/explorer_system_prompt.md").to_string()
}

pub fn general_system_prompt() -> String {
    include_str!("prompts/general_system_prompt.md").to_string()
}
