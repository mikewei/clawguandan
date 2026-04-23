//! Per-phase bot policies (`Arc<dyn ...>`) composed by [`super::plugin::BotPlugin`].

mod name_policy;
mod observer_policy;
mod ready_policy;
mod suggest_policy;
mod traits;

pub use name_policy::{DefaultNamePolicy, default_display_names_for_plugin, default_name};
pub use observer_policy::{DefaultObserverPolicy, default_observer};
pub use ready_policy::{AlwaysReadyPolicy, always_ready};
pub use suggest_policy::{
    AlwaysSuggestPolicy, always_suggest_exchange, always_suggest_play, always_suggest_tribute,
};
pub use traits::{
    ExchangePolicy, NamePolicy, ObserverGameOverContext, ObserverGameStartContext,
    ObserverHandOverContext, ObserverHandStartContext, ObserverPolicy, PlayPolicy, ReadyPolicy,
    TributePolicy,
};
