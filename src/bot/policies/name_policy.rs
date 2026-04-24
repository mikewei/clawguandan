use std::sync::{Arc, OnceLock};

use crate::bot::plugin::JoinNamesContext;

use super::traits::NamePolicy;

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultNamePolicy;

impl NamePolicy for DefaultNamePolicy {
    fn join_display_names(&self, ctx: &JoinNamesContext) -> Result<Vec<String>, String> {
        Ok(default_display_names_for_plugin(&ctx.plugin_id, ctx.count))
    }
}

pub fn default_name() -> Arc<dyn NamePolicy> {
    static CELL: OnceLock<Arc<DefaultNamePolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(DefaultNamePolicy)).clone() as Arc<dyn NamePolicy>
}

pub fn default_display_names_for_plugin(plugin_id: &str, count: usize) -> Vec<String> {
    let prefix = plugin_display_prefix(plugin_id);
    (0..count).map(|i| format!("{prefix}{i}")).collect()
}

fn plugin_display_prefix(plugin_id: &str) -> String {
    let mut out = String::new();
    for token in plugin_id
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        let mut chars = token.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            for ch in chars {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    if out.is_empty() {
        "Bot".to_string()
    } else {
        out
    }
}
