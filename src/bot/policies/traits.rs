use serde_json::Value;

use crate::bot::plugin::{BotDecision, BotTurnContext, JoinNamesContext};

pub trait ReadyPolicy: Send + Sync {
    fn decide_ready(&self, ctx: &BotTurnContext) -> Result<BotDecision, String>;
}

pub trait TributePolicy: Send + Sync {
    fn decide_tribute(&self, ctx: &BotTurnContext) -> Result<BotDecision, String>;
}

pub trait ExchangePolicy: Send + Sync {
    fn decide_exchange(&self, ctx: &BotTurnContext) -> Result<BotDecision, String>;
}

pub trait PlayPolicy: Send + Sync {
    fn decide_play(&self, ctx: &BotTurnContext) -> Result<BotDecision, String>;
}

pub trait NamePolicy: Send + Sync {
    fn join_display_names(&self, ctx: &JoinNamesContext) -> Result<Vec<String>, String>;
}

#[derive(Clone, Debug)]
pub struct ObserverGameStartContext {
    pub plugin_id: String,
    pub table_id: String,
    pub observer_name: String,
    pub transition_seq: u64,
    pub hands_target: Option<u32>,
    pub occupied: usize,
    pub vacancy: usize,
    pub join_bots: usize,
    pub verbosity: u8,
}

#[derive(Clone, Debug)]
pub struct ObserverHandStartContext {
    pub plugin_id: String,
    pub table_id: String,
    pub hand_index: u32,
    pub transition_seq: u64,
    pub transition_type: String,
    pub verbosity: u8,
}

#[derive(Clone, Debug)]
pub struct ObserverHandOverContext {
    pub plugin_id: String,
    pub table_id: String,
    pub hand_index: u32,
    pub transition_seq: u64,
    pub transition_type: String,
    pub verbosity: u8,
}

#[derive(Clone, Debug)]
pub struct ObserverGameOverContext {
    pub plugin_id: String,
    pub table_id: String,
    pub hands_done: u32,
    pub transition_seq: u64,
    pub transition_type: String,
    pub verbosity: u8,
}

pub trait ObserverPolicy: Send + Sync {
    fn on_transition(&self, _transition: &Value, _verbosity: u8) -> Result<(), String> {
        Ok(())
    }

    fn on_game_start(&self, _ctx: &ObserverGameStartContext) -> Result<(), String> {
        Ok(())
    }

    fn on_hand_start(&self, _ctx: &ObserverHandStartContext) -> Result<(), String> {
        Ok(())
    }

    fn on_hand_over(&self, _ctx: &ObserverHandOverContext) -> Result<(), String> {
        Ok(())
    }

    fn on_game_over(&self, _ctx: &ObserverGameOverContext) -> Result<(), String> {
        Ok(())
    }
}
