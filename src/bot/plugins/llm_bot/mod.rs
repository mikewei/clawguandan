//! `llm-bot` plugin: user script (stdin prompt, stdout markers).

mod name_policy;
mod naming;
mod parse;
mod prompt;
mod script;
mod script_policy;
mod self_check;

use std::sync::Arc;

use std::time::Duration;

use crate::bot::plugin::BotPlugin;
use crate::bot::policies::{ExchangePolicy, NamePolicy, PlayPolicy, TributePolicy};

use self::name_policy::LlmNamePolicy;
use self::script_policy::LlmScriptPolicy;
pub use self::self_check::{resolve_join_model, verify_script_model};

#[derive(Clone, Debug)]
pub struct LlmBotParams {
    pub script: std::path::PathBuf,
    pub timeout: Duration,
    pub name_bots: bool,
    pub model: Option<String>,
    /// CLI verbosity count (`-v`, `-vv`, ...).
    pub verbosity: u8,
}

#[derive(Clone, Debug)]
pub struct LlmBotPlugin {
    script: Arc<LlmScriptPolicy>,
    name: Arc<LlmNamePolicy>,
}

impl LlmBotPlugin {
    pub fn new(params: LlmBotParams) -> Self {
        let params = Arc::new(params);
        Self {
            script: Arc::new(LlmScriptPolicy {
                params: Arc::clone(&params),
            }),
            name: Arc::new(LlmNamePolicy { params }),
        }
    }
}

impl BotPlugin for LlmBotPlugin {
    fn plugin_id(&self) -> &'static str {
        "llm-bot"
    }

    fn play_policy(&self) -> Arc<dyn PlayPolicy> {
        self.script.clone() as Arc<dyn PlayPolicy>
    }

    fn tribute_policy(&self) -> Arc<dyn TributePolicy> {
        self.script.clone() as Arc<dyn TributePolicy>
    }

    fn exchange_policy(&self) -> Arc<dyn ExchangePolicy> {
        self.script.clone() as Arc<dyn ExchangePolicy>
    }

    fn name_policy(&self) -> Arc<dyn NamePolicy> {
        self.name.clone() as Arc<dyn NamePolicy>
    }

    fn join_player_model(&self) -> Option<String> {
        self.script.params.model.clone()
    }
}
