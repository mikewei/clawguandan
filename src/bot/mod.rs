pub mod plugin;
pub mod policies;
pub mod plugins;
pub mod runtime;

pub use plugin::{BotDecision, BotPlugin, BotTurnContext, JoinNamesContext};
pub use policies::{
    always_ready, always_suggest_exchange, always_suggest_play, always_suggest_tribute,
    default_display_names_for_plugin, default_name, default_observer, AlwaysReadyPolicy,
    AlwaysSuggestPolicy, DefaultNamePolicy, DefaultObserverPolicy, ExchangePolicy, NamePolicy,
    ObserverGameOverContext, ObserverGameStartContext, ObserverHandOverContext,
    ObserverHandStartContext, ObserverPolicy, PlayPolicy, ReadyPolicy, TributePolicy,
};
pub use runtime::{BotRunOptions, run_bot_subprocess};
