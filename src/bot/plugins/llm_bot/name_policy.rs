use std::sync::Arc;

use crate::bot::plugin::JoinNamesContext;
use crate::bot::policies::NamePolicy;

use super::naming;
use super::LlmBotParams;

#[derive(Debug)]
pub struct LlmNamePolicy {
    pub(crate) params: Arc<LlmBotParams>,
}

impl NamePolicy for LlmNamePolicy {
    fn join_display_names(&self, ctx: &JoinNamesContext) -> Result<Vec<String>, String> {
        if !self.params.name_bots {
            return Err("llm naming disabled".into());
        }
        naming::resolve(
            &self.params.script,
            self.params.timeout,
            self.params.verbosity,
            ctx,
        )
    }
}
