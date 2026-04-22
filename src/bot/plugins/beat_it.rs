use crate::bot::plugin::{BotDecision, BotPlugin, BotTurnContext};

#[derive(Clone, Debug, Default)]
pub struct BeatItPlugin;

impl BotPlugin for BeatItPlugin {
    fn name(&self) -> &'static str {
        "beat-it"
    }

    fn observer_prefix(&self) -> &'static str {
        "bi"
    }

    fn decide(&self, ctx: &BotTurnContext) -> Result<BotDecision, String> {
        if ctx.expect_kind == "ready" {
            return Ok(BotDecision::Ready);
        }
        Ok(BotDecision::UseSuggest)
    }
}
