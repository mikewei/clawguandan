//! Strategy trait: choose one of the precomputed legal actions.

use crate::game::engine::PlayerAction;
use rand::Rng;

/// Choose a [`PlayerAction`] from `legal`, which should come from [`crate::strategy::movegen::enumerate_legal_actions`].
pub trait GameStrategy {
    fn choose<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
        legal: &[PlayerAction],
    ) -> Option<PlayerAction>;
}
