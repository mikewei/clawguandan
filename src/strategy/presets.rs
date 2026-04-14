//! Built-in strategies.

use rand::Rng;
use rand::seq::IndexedRandom;

use crate::game::engine::PlayerAction;
use crate::strategy::traits::GameStrategy;

/// Deterministic: first action in slice order (stable if `legal` is stable).
#[derive(Clone, Copy, Debug, Default)]
pub struct FirstLegal;

impl GameStrategy for FirstLegal {
    fn choose<R: Rng + ?Sized>(
        &mut self,
        _rng: &mut R,
        legal: &[PlayerAction],
    ) -> Option<PlayerAction> {
        legal.first().cloned()
    }
}

/// Uniform random choice among legal actions.
#[derive(Clone, Debug, Default)]
pub struct RandomLegal;

impl GameStrategy for RandomLegal {
    fn choose<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
        legal: &[PlayerAction],
    ) -> Option<PlayerAction> {
        legal.choose(rng).cloned()
    }
}
