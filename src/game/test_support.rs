use crate::domain::Seat;
use crate::game::card::{Card, HandLevel};
use crate::game::deck::Deck;
use crate::game::types::{GamePhase, HandState, TableGameState, TributePair, TributeState};

pub struct TestFixtures;

impl TestFixtures {
    pub fn deck(seed: u64) -> Deck {
        Deck::new_shuffled_double_deck(seed)
    }

    pub fn dealt_27_each(seed: u64, first: Seat) -> std::collections::HashMap<Seat, Vec<Card>> {
        Self::deck(seed).deal_27_each_ccw_from(first)
    }

    pub fn table_id() -> String {
        "t_test".to_string()
    }

    /// Two payers (W,N) tribute to (E,S); hands sized for unit/integration tests only.
    pub fn table_game_tribute_two_pairs() -> TableGameState {
        let mut s = TableGameState::new("t_rig".into());
        s.phase = GamePhase::Tribute;
        s.hand_index = 2;
        s.dealer_seat = Seat::E;
        s.leader_seat = Seat::E;
        s.turn_seat = Seat::E;
        let mut hand = HandState::new(HandLevel::Two);
        hand.hands.insert(Seat::E, vec!["♦5".into(), "♣6".into()]);
        hand.hands.insert(Seat::S, vec!["♠8".into(), "♦9".into()]);
        hand.hands.insert(Seat::W, vec!["♠A".into(), "♣3".into()]);
        hand.hands.insert(Seat::N, vec!["♦K".into(), "♥4".into()]);
        hand.tribute = Some(TributeState {
            pairs: vec![
                TributePair {
                    payer: Seat::W,
                    receiver: Seat::E,
                    paid_card: None,
                    return_card: None,
                },
                TributePair {
                    payer: Seat::N,
                    receiver: Seat::S,
                    paid_card: None,
                    return_card: None,
                },
            ],
            canceled: false,
            opening_lead_candidates: vec![Seat::W, Seat::N],
        });
        s.hand = Some(hand);
        s
    }

    /// Four singles (E/W/S/N); playing with E to lead. Scripted play/pass reaches `Scoring` with EW winning.
    pub fn table_game_playing_four_singles_endgame() -> TableGameState {
        let mut s = TableGameState::new("t_rig".into());
        s.phase = GamePhase::Playing;
        s.hand_index = 1;
        s.dealer_seat = Seat::E;
        s.leader_seat = Seat::E;
        s.turn_seat = Seat::E;
        let mut hand = HandState::new(HandLevel::Two);
        hand.hands.insert(Seat::E, vec!["♠3".into()]);
        hand.hands.insert(Seat::W, vec!["♠4".into()]);
        hand.hands.insert(Seat::S, vec!["♠5".into()]);
        hand.hands.insert(Seat::N, vec!["♠6".into()]);
        s.hand = Some(hand);
        s
    }
}
