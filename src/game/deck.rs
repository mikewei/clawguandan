use rand::SeedableRng;
use rand::seq::SliceRandom;

use crate::domain::Seat;
use crate::game::card::{Card, Rank, Suit};

/// Deterministic deck builder for tests and reproducible dealing.
pub struct Deck {
    cards: Vec<Card>,
}

impl Deck {
    pub fn new_double_deck() -> Self {
        let mut single = Vec::with_capacity(54);
        for suit in [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs] {
            for rank in [
                Rank::A,
                Rank::K,
                Rank::Q,
                Rank::J,
                Rank::Ten,
                Rank::Nine,
                Rank::Eight,
                Rank::Seven,
                Rank::Six,
                Rank::Five,
                Rank::Four,
                Rank::Three,
                Rank::Two,
            ] {
                single.push(Card { suit, rank });
            }
        }
        single.push(Card {
            suit: Suit::Joker,
            rank: Rank::BlackJoker,
        });
        single.push(Card {
            suit: Suit::Joker,
            rank: Rank::RedJoker,
        });

        let mut cards = Vec::with_capacity(108);
        cards.extend_from_slice(&single);
        cards.extend_from_slice(&single);
        Self { cards }
    }

    pub fn new_shuffled_double_deck(seed: u64) -> Self {
        let mut d = Self::new_double_deck();
        d.shuffle_with_seed(seed);
        d
    }

    pub fn shuffle_with_seed(&mut self, seed: u64) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        self.cards.shuffle(&mut rng);
    }

    /// Deterministically deal 27 cards to each seat in counterclockwise order.
    /// This matches the design constraint of reproducible transitions; the
    /// specific dealing policy can be refined later without affecting tests that
    /// rely on the injected seed.
    pub fn deal_27_each_ccw_from(&self, first: Seat) -> std::collections::HashMap<Seat, Vec<Card>> {
        let mut out: std::collections::HashMap<Seat, Vec<Card>> = std::collections::HashMap::new();
        for s in Seat::ALL {
            out.insert(s, Vec::with_capacity(27));
        }
        let order = ccw_seat_order_from(first);
        for (i, card) in self.cards.iter().copied().enumerate() {
            let seat = order[i % 4];
            out.get_mut(&seat).expect("seat exists").push(card);
        }
        out
    }

    pub fn as_slice(&self) -> &[Card] {
        &self.cards
    }
}

fn ccw_seat_order_from(first: Seat) -> [Seat; 4] {
    // Counterclockwise per doc: deal/play are counterclockwise.
    // We use the fixed seat circle E -> N -> W -> S -> E.
    fn next_ccw(s: Seat) -> Seat {
        match s {
            Seat::E => Seat::N,
            Seat::N => Seat::W,
            Seat::W => Seat::S,
            Seat::S => Seat::E,
        }
    }
    let s1 = first;
    let s2 = next_ccw(s1);
    let s3 = next_ccw(s2);
    let s4 = next_ccw(s3);
    [s1, s2, s3, s4]
}
