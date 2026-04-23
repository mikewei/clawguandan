use std::sync::{Arc, OnceLock};

use crate::bot::plugin::{BotDecision, BotTurnContext};

use super::traits::{ExchangePolicy, PlayPolicy, TributePolicy};

#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysSuggestPolicy;

impl TributePolicy for AlwaysSuggestPolicy {
    fn decide_tribute(&self, _ctx: &BotTurnContext) -> Result<BotDecision, String> {
        Ok(BotDecision::UseSuggest)
    }
}

impl ExchangePolicy for AlwaysSuggestPolicy {
    fn decide_exchange(&self, _ctx: &BotTurnContext) -> Result<BotDecision, String> {
        Ok(BotDecision::UseSuggest)
    }
}

impl PlayPolicy for AlwaysSuggestPolicy {
    fn decide_play(&self, _ctx: &BotTurnContext) -> Result<BotDecision, String> {
        Ok(BotDecision::UseSuggest)
    }
}

fn always_suggest_inner() -> Arc<AlwaysSuggestPolicy> {
    static CELL: OnceLock<Arc<AlwaysSuggestPolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(AlwaysSuggestPolicy)).clone()
}

pub fn always_suggest_tribute() -> Arc<dyn TributePolicy> {
    always_suggest_inner() as Arc<dyn TributePolicy>
}

pub fn always_suggest_exchange() -> Arc<dyn ExchangePolicy> {
    always_suggest_inner() as Arc<dyn ExchangePolicy>
}

pub fn always_suggest_play() -> Arc<dyn PlayPolicy> {
    always_suggest_inner() as Arc<dyn PlayPolicy>
}
