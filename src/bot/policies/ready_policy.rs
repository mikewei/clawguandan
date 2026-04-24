use std::sync::{Arc, OnceLock};

use crate::bot::plugin::BotDecision;

use super::traits::ReadyPolicy;

#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysReadyPolicy;

impl ReadyPolicy for AlwaysReadyPolicy {
    fn decide_ready(
        &self,
        _ctx: &crate::bot::plugin::BotTurnContext,
    ) -> Result<BotDecision, String> {
        Ok(BotDecision::Ready)
    }
}

fn always_ready_inner() -> Arc<AlwaysReadyPolicy> {
    static CELL: OnceLock<Arc<AlwaysReadyPolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(AlwaysReadyPolicy)).clone()
}

pub fn always_ready() -> Arc<dyn ReadyPolicy> {
    always_ready_inner() as Arc<dyn ReadyPolicy>
}
