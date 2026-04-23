//! Per-phase bot policies (`Arc<dyn …>`) composed by [`super::plugin::BotPlugin`].

use std::sync::{Arc, OnceLock};

use serde_json::Value;

use super::plugin::{BotDecision, BotTurnContext, JoinNamesContext};

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

pub trait ObserverPolicy: Send + Sync {
    fn on_transition(&self, transition: &Value) -> Result<(), String>;
}

// --- Default ZST policies ---

#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysReadyPolicy;

impl ReadyPolicy for AlwaysReadyPolicy {
    fn decide_ready(&self, _ctx: &BotTurnContext) -> Result<BotDecision, String> {
        Ok(BotDecision::Ready)
    }
}

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

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultNamePolicy;

impl NamePolicy for DefaultNamePolicy {
    fn join_display_names(&self, ctx: &JoinNamesContext) -> Result<Vec<String>, String> {
        Ok(default_display_names_for_plugin(&ctx.plugin_id, ctx.count))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpObserverPolicy;

impl ObserverPolicy for NoOpObserverPolicy {
    fn on_transition(&self, _transition: &Value) -> Result<(), String> {
        Ok(())
    }
}

// --- Shared Arc singletons (clone is cheap refcount) ---

fn always_ready_inner() -> Arc<AlwaysReadyPolicy> {
    static CELL: OnceLock<Arc<AlwaysReadyPolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(AlwaysReadyPolicy)).clone()
}

fn always_suggest_inner() -> Arc<AlwaysSuggestPolicy> {
    static CELL: OnceLock<Arc<AlwaysSuggestPolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(AlwaysSuggestPolicy)).clone()
}

pub fn always_ready() -> Arc<dyn ReadyPolicy> {
    always_ready_inner() as Arc<dyn ReadyPolicy>
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

pub fn default_name() -> Arc<dyn NamePolicy> {
    static CELL: OnceLock<Arc<DefaultNamePolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(DefaultNamePolicy))
        .clone() as Arc<dyn NamePolicy>
}

pub fn noop_observer() -> Arc<dyn ObserverPolicy> {
    static CELL: OnceLock<Arc<NoOpObserverPolicy>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(NoOpObserverPolicy))
        .clone() as Arc<dyn ObserverPolicy>
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
