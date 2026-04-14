//! Play strategy: legal move generation and pluggable choosers for tests and automation.
//!
//! Depends on [`crate::game`] only; the game crate does not depend on this module.

pub mod movegen;
pub mod observation;
pub mod presets;
pub mod suggest;
pub mod traits;

pub use movegen::{current_actor_seat, enumerate_legal_actions};
pub use observation::StrategyObservation;
pub use presets::{FirstLegal, RandomLegal};
pub use suggest::suggest_next_action;
pub use traits::GameStrategy;
