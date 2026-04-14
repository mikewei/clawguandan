//! View model for strategy: can be filled from [`crate::game::types::TableGameState`]
//! or from HTTP/runtime snapshots that carry the same facts.

use crate::domain::Seat;
use crate::game::card::HandLevel;
use crate::game::rules::combination_parser::Combination;
use crate::game::types::{GamePhase, TableGameState};

use super::movegen::current_actor_seat;

/// Information needed to choose a legal [`crate::game::engine::PlayerAction`].
#[derive(Clone, Debug)]
pub struct StrategyObservation {
    pub phase: GamePhase,
    pub turn_seat: Seat,
    pub actor_hand: Vec<String>,
    pub hand_level: HandLevel,
    /// When in `Playing`, the combination to beat (if any).
    pub trick_top: Option<Combination>,
}

impl StrategyObservation {
    /// Full-information view from engine state for `actor` (must be `state.turn_seat`).
    pub fn from_table_game_state(state: &TableGameState, actor: Seat) -> Option<Self> {
        if current_actor_seat(state) != Some(actor) {
            return None;
        }
        let hand = state.hand.as_ref()?;
        let actor_hand = hand.hands.get(&actor).cloned().unwrap_or_default();
        let trick_top = hand.trick.top_play.as_ref().map(|p| p.combination.clone());
        Some(Self {
            phase: state.phase,
            turn_seat: actor,
            actor_hand,
            hand_level: hand.hand_level,
            trick_top,
        })
    }
}
