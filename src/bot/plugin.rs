use crate::game::engine::PlayerAction;
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum BotDecision {
    Ready,
    UseSuggest,
    Action(PlayerAction),
}

#[derive(Clone, Debug)]
pub struct BotTurnContext {
    pub table_id: String,
    pub player_id: String,
    pub expect_kind: String,
    pub state: Value,
}

pub trait BotPlugin: Send + Sync {
    fn name(&self) -> &'static str;

    fn observer_prefix(&self) -> &'static str {
        "bot"
    }

    fn decide(&self, ctx: &BotTurnContext) -> Result<BotDecision, String>;

    fn on_observer_transition(&self, _transition: &Value) -> Result<(), String> {
        Ok(())
    }
}
