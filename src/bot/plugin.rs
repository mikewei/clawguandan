use std::sync::Arc;

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

/// Context for optional per-plugin join display names.
#[derive(Clone, Debug)]
pub struct JoinNamesContext {
    pub plugin_id: String,
    pub table_id: String,
    pub count: usize,
    pub snapshot: Option<Value>,
}

pub trait BotPlugin: Send + Sync {
    /// Stable machine-facing id (session dirs, logging); not the same as join display names.
    fn plugin_id(&self) -> &'static str;

    fn ready_policy(&self) -> Arc<dyn crate::bot::policies::ReadyPolicy> {
        crate::bot::policies::always_ready()
    }

    fn tribute_policy(&self) -> Arc<dyn crate::bot::policies::TributePolicy> {
        crate::bot::policies::always_suggest_tribute()
    }

    fn exchange_policy(&self) -> Arc<dyn crate::bot::policies::ExchangePolicy> {
        crate::bot::policies::always_suggest_exchange()
    }

    fn play_policy(&self) -> Arc<dyn crate::bot::policies::PlayPolicy> {
        crate::bot::policies::always_suggest_play()
    }

    fn name_policy(&self) -> Arc<dyn crate::bot::policies::NamePolicy> {
        crate::bot::policies::default_name()
    }

    fn observer_policy(&self) -> Arc<dyn crate::bot::policies::ObserverPolicy> {
        crate::bot::policies::default_observer()
    }

    /// Optional join-time `playerModel` to pass into `table join --model`.
    fn join_player_model(&self) -> Option<String> {
        None
    }
}
