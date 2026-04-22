use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Suit {
    Spades,
    Hearts,
    Diamonds,
    Clubs,
    Joker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rank {
    A,
    K,
    Q,
    J,
    Ten,
    Nine,
    Eight,
    Seven,
    Six,
    Five,
    Four,
    Three,
    Two,
    BlackJoker,
    RedJoker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Card {
    pub suit: Suit,
    pub rank: Rank,
}

/// Compact symbol used by API and prompts, e.g. `♠A`, `♦10`, `🃏R`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardSymbol(pub String);

impl CardSymbol {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandLevel {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    J,
    Q,
    K,
    A,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleContext {
    pub hand_level: HandLevel,
}

impl HandLevel {
    pub fn to_rank(self) -> Rank {
        match self {
            HandLevel::Two => Rank::Two,
            HandLevel::Three => Rank::Three,
            HandLevel::Four => Rank::Four,
            HandLevel::Five => Rank::Five,
            HandLevel::Six => Rank::Six,
            HandLevel::Seven => Rank::Seven,
            HandLevel::Eight => Rank::Eight,
            HandLevel::Nine => Rank::Nine,
            HandLevel::Ten => Rank::Ten,
            HandLevel::J => Rank::J,
            HandLevel::Q => Rank::Q,
            HandLevel::K => Rank::K,
            HandLevel::A => Rank::A,
        }
    }

    /// API / display rank for the current hand level (e.g. `"10"`, `"A"`).
    pub fn as_api_str(self) -> &'static str {
        match self {
            HandLevel::Two => "2",
            HandLevel::Three => "3",
            HandLevel::Four => "4",
            HandLevel::Five => "5",
            HandLevel::Six => "6",
            HandLevel::Seven => "7",
            HandLevel::Eight => "8",
            HandLevel::Nine => "9",
            HandLevel::Ten => "10",
            HandLevel::J => "J",
            HandLevel::Q => "Q",
            HandLevel::K => "K",
            HandLevel::A => "A",
        }
    }
}

pub fn parse_card_symbol(sym: &str) -> Result<Card, String> {
    let s = sym.trim();
    if s == "🃏R" {
        return Ok(Card {
            suit: Suit::Joker,
            rank: Rank::RedJoker,
        });
    }
    if s == "🃏b" {
        return Ok(Card {
            suit: Suit::Joker,
            rank: Rank::BlackJoker,
        });
    }

    let (suit, rest) = if let Some(r) = s.strip_prefix("♠") {
        (Suit::Spades, r)
    } else if let Some(r) = s.strip_prefix("♥") {
        (Suit::Hearts, r)
    } else if let Some(r) = s.strip_prefix("♦") {
        (Suit::Diamonds, r)
    } else if let Some(r) = s.strip_prefix("♣") {
        (Suit::Clubs, r)
    } else {
        return Err(format!("invalid card symbol {:?}", sym));
    };

    let rank = match rest {
        "A" => Rank::A,
        "K" => Rank::K,
        "Q" => Rank::Q,
        "J" => Rank::J,
        "10" => Rank::Ten,
        "9" => Rank::Nine,
        "8" => Rank::Eight,
        "7" => Rank::Seven,
        "6" => Rank::Six,
        "5" => Rank::Five,
        "4" => Rank::Four,
        "3" => Rank::Three,
        "2" => Rank::Two,
        _ => return Err(format!("invalid rank in symbol {:?}", sym)),
    };

    Ok(Card { suit, rank })
}

pub fn to_card_symbol(card: Card) -> String {
    match (card.suit, card.rank) {
        (Suit::Joker, Rank::RedJoker) => "🃏R".to_string(),
        (Suit::Joker, Rank::BlackJoker) => "🃏b".to_string(),
        (suit, rank) => {
            let suit_s = match suit {
                Suit::Spades => "♠",
                Suit::Hearts => "♥",
                Suit::Diamonds => "♦",
                Suit::Clubs => "♣",
                Suit::Joker => unreachable!(),
            };
            let rank_s = match rank {
                Rank::A => "A",
                Rank::K => "K",
                Rank::Q => "Q",
                Rank::J => "J",
                Rank::Ten => "10",
                Rank::Nine => "9",
                Rank::Eight => "8",
                Rank::Seven => "7",
                Rank::Six => "6",
                Rank::Five => "5",
                Rank::Four => "4",
                Rank::Three => "3",
                Rank::Two => "2",
                Rank::BlackJoker | Rank::RedJoker => unreachable!(),
            };
            format!("{}{}", suit_s, rank_s)
        }
    }
}

pub fn is_wild(card: Card, ctx: RuleContext) -> bool {
    card.suit == Suit::Hearts && card.rank == ctx.hand_level.to_rank()
}

pub fn natural_rank_value(rank: Rank) -> Result<u8, String> {
    Ok(match rank {
        Rank::Two => 2,
        Rank::Three => 3,
        Rank::Four => 4,
        Rank::Five => 5,
        Rank::Six => 6,
        Rank::Seven => 7,
        Rank::Eight => 8,
        Rank::Nine => 9,
        Rank::Ten => 10,
        Rank::J => 11,
        Rank::Q => 12,
        Rank::K => 13,
        Rank::A => 14,
        Rank::BlackJoker | Rank::RedJoker => {
            return Err("joker has no natural rank for sequences".into());
        }
    })
}

pub fn level_order_value(card: Card, ctx: RuleContext) -> u8 {
    match card.rank {
        Rank::RedJoker => 16,
        Rank::BlackJoker => 15,
        _ => {
            if card.rank == ctx.hand_level.to_rank() {
                14
            } else {
                match card.rank {
                    Rank::A => 13,
                    Rank::K => 12,
                    Rank::Q => 11,
                    Rank::J => 10,
                    Rank::Ten => 9,
                    Rank::Nine => 8,
                    Rank::Eight => 7,
                    Rank::Seven => 6,
                    Rank::Six => 5,
                    Rank::Five => 4,
                    Rank::Four => 3,
                    Rank::Three => 2,
                    Rank::Two => 1,
                    Rank::BlackJoker | Rank::RedJoker => unreachable!(),
                }
            }
        }
    }
}

fn suit_weight_desc(suit: Suit) -> u8 {
    match suit {
        Suit::Hearts => 4,
        Suit::Spades => 3,
        Suit::Diamonds => 2,
        Suit::Clubs => 1,
        Suit::Joker => 0,
    }
}

/// Sort card symbols in descending order using current hand-level rules.
/// Unknown symbols are placed at the end, preserving relative order among valid cards.
pub fn sort_card_symbols_desc(card_symbols: &mut [String], hand_level: HandLevel) {
    let ctx = RuleContext { hand_level };
    card_symbols.sort_by(|a, b| {
        let a_card = parse_card_symbol(a).ok();
        let b_card = parse_card_symbol(b).ok();
        match (a_card, b_card) {
            (Some(a_card), Some(b_card)) => {
                let a_rank = level_order_value(a_card, ctx);
                let b_rank = level_order_value(b_card, ctx);
                b_rank
                    .cmp(&a_rank)
                    .then_with(|| suit_weight_desc(b_card.suit).cmp(&suit_weight_desc(a_card.suit)))
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}
