//! Drive [`GameEngine`] until scoring (optionally multiple hands).

use crate::domain::Seat;
use crate::game::card::HandLevel;
use crate::game::engine::GameEngine;
use crate::game::types::{GamePhase, TableGameState, TeamId};
use crate::strategy::{current_actor_seat, suggest_next_action};

#[derive(Clone, Debug)]
pub enum EngineSimError {
    NoLegalActions { seat: Seat, phase: GamePhase },
    Engine(String),
    Movegen(String),
}

impl std::fmt::Display for EngineSimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineSimError::NoLegalActions { seat, phase } => {
                write!(f, "no legal actions at {:?} phase {:?}", seat, phase)
            }
            EngineSimError::Engine(s) | EngineSimError::Movegen(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for EngineSimError {}

/// Result of [`run_match_engine`].
#[derive(Clone, Debug)]
pub struct EngineSimOutcome {
    pub hands_played: u32,
    pub final_phase: GamePhase,
}

/// Run from an in-progress [`TableGameState`] until `Scoring`, then optionally start the next hand with tribute.
///
/// Uses [`crate::strategy::suggest_next_action`] (same policy as HTTP GET `/suggest`).
pub fn run_match_engine(
    engine: &GameEngine,
    state: &mut TableGameState,
    num_hands: u32,
    max_plies: usize,
) -> Result<EngineSimOutcome, EngineSimError> {
    if num_hands == 0 {
        return Ok(EngineSimOutcome {
            hands_played: 0,
            final_phase: state.phase,
        });
    }

    let mut hands_played = 0u32;
    let mut plies = 0usize;

    loop {
        if hands_played >= num_hands {
            return Ok(EngineSimOutcome {
                hands_played,
                final_phase: state.phase,
            });
        }

        loop {
            if plies >= max_plies {
                return Err(EngineSimError::Engine(
                    "max_plies exceeded".to_string(),
                ));
            }
            if state.phase == GamePhase::Scoring {
                hands_played += 1;
                break;
            }
            if !matches!(
                state.phase,
                GamePhase::Playing | GamePhase::Tribute | GamePhase::Exchange
            ) {
                return Err(EngineSimError::Engine(format!(
                    "unexpected phase {:?}",
                    state.phase
                )));
            }

            let Some(actor) = current_actor_seat(state) else {
                return Err(EngineSimError::Engine(
                    "no current actor for phase".into(),
                ));
            };
            let action = suggest_next_action(state, actor).map_err(EngineSimError::Movegen)?;

            engine
                .apply_player_action(state, actor, action, None)
                .map_err(EngineSimError::Engine)?;
            plies += 1;
        }

        if hands_played >= num_hands {
            return Ok(EngineSimOutcome {
                hands_played,
                final_phase: state.phase,
            });
        }

        let finishing = state
            .hand
            .as_ref()
            .map(|h| h.finishing_order.clone())
            .filter(|o| o.len() == 4)
            .ok_or_else(|| {
                EngineSimError::Engine("missing finishing_order for next hand".into())
            })?;
        let declarer = state.winner_team.unwrap_or(TeamId::Ew);
        engine
            .start_next_hand_with_tribute(state, declarer, HandLevel::Two, &finishing)
            .map_err(EngineSimError::Engine)?;
    }
}
