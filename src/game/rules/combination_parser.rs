use std::collections::HashMap;

use crate::game::card::{Card, Rank, RuleContext, Suit, level_order_value, natural_rank_value};
use crate::game::rules::wildcard_resolver::WildcardResolver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombinationClass {
    Ordinary,
    Bomb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryKind {
    Single,
    Pair,
    Triple,
    FullHouse,
    Straight,
    Tube,
    Plate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BombKind {
    SameRank { n: u8 }, // 4..=10
    StraightFlush,      // 5
    FourJoker,          // 4
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombinationKind {
    Ordinary(OrdinaryKind),
    Bomb(BombKind),
}

/// API `combinationType` string for [`crate::game::types::HandHistoryEntry`].
pub fn combination_kind_api_type(kind: &CombinationKind) -> String {
    match kind {
        CombinationKind::Ordinary(OrdinaryKind::Single) => "single".into(),
        CombinationKind::Ordinary(OrdinaryKind::Pair) => "pair".into(),
        CombinationKind::Ordinary(OrdinaryKind::Triple) => "triple".into(),
        CombinationKind::Ordinary(OrdinaryKind::FullHouse) => "fullHouse".into(),
        CombinationKind::Ordinary(OrdinaryKind::Straight) => "straight".into(),
        CombinationKind::Ordinary(OrdinaryKind::Tube) => "tube".into(),
        CombinationKind::Ordinary(OrdinaryKind::Plate) => "plate".into(),
        CombinationKind::Bomb(BombKind::SameRank { n }) => format!("bomb{n}"),
        CombinationKind::Bomb(BombKind::StraightFlush) => "straightFlush".into(),
        CombinationKind::Bomb(BombKind::FourJoker) => "fourJoker".into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combination {
    pub kind: CombinationKind,
    pub cards_len: u8,
    /// Main comparable value; semantics depend on kind:
    /// - ordinary(single/pair/triple/full_house/same-rank bomb): level-order primary rank value
    /// - straight/tube/plate/straight_flush: highest natural rank (with A-low handled)
    pub primary: u8,
    /// For bombs: tier from low to high (quad=1, quint=2, straight_flush=3, sext..decuple=4..8, four_joker=9)
    pub bomb_tier: u8,
}

impl Combination {
    pub fn class(&self) -> CombinationClass {
        match self.kind {
            CombinationKind::Ordinary(_) => CombinationClass::Ordinary,
            CombinationKind::Bomb(_) => CombinationClass::Bomb,
        }
    }
}

pub struct CombinationParser;

impl CombinationParser {
    pub fn parse(
        cards: &[String],
        wild_targets: Option<&[String]>,
        ctx: RuleContext,
    ) -> Result<Combination, String> {
        if cards.is_empty() {
            return Err("empty cards".into());
        }

        // Resolve wildcards first; this both validates declared mapping and yields a concrete shape.
        let resolved = WildcardResolver::resolve(cards, wild_targets, ctx)?;
        let n = resolved.len();

        // Four-joker bomb (two red + two black) — must be 4 cards.
        if n == 4 && is_four_joker(&resolved) {
            return Ok(Combination {
                kind: CombinationKind::Bomb(BombKind::FourJoker),
                cards_len: 4,
                primary: 0,
                bomb_tier: 9,
            });
        }

        // Same-rank bombs 4..=10 (jokers allowed as ranks, but there aren't enough for 4+ anyway).
        if (4..=10).contains(&n) && all_same_rank(&resolved) {
            let primary = level_order_value(resolved[0], ctx);
            let bomb_tier = match n {
                4 => 1,
                5 => 2,
                6 => 4,
                7 => 5,
                8 => 6,
                9 => 7,
                10 => 8,
                _ => return Err("invalid same-rank bomb length".into()),
            };
            return Ok(Combination {
                kind: CombinationKind::Bomb(BombKind::SameRank { n: n as u8 }),
                cards_len: n as u8,
                primary,
                bomb_tier,
            });
        }

        // Straight flush bomb: 5 cards, same suit, consecutive in natural order (no jokers).
        if n == 5 && is_straight_flush(&resolved)? {
            let hi = straight_highest_natural(&resolved)?;
            return Ok(Combination {
                kind: CombinationKind::Bomb(BombKind::StraightFlush),
                cards_len: 5,
                primary: hi,
                bomb_tier: 3,
            });
        }

        // Ordinary types.
        match n {
            1 => Ok(Combination {
                kind: CombinationKind::Ordinary(OrdinaryKind::Single),
                cards_len: 1,
                primary: level_order_value(resolved[0], ctx),
                bomb_tier: 0,
            }),
            2 => {
                ensure_no_mixed_joker_pair(&resolved)?;
                ensure_same_rank(&resolved)?;
                Ok(Combination {
                    kind: CombinationKind::Ordinary(OrdinaryKind::Pair),
                    cards_len: 2,
                    primary: level_order_value(resolved[0], ctx),
                    bomb_tier: 0,
                })
            }
            3 => {
                ensure_same_rank(&resolved)?;
                if resolved.iter().any(|c| c.suit == Suit::Joker) {
                    return Err("joker triple is not possible".into());
                }
                Ok(Combination {
                    kind: CombinationKind::Ordinary(OrdinaryKind::Triple),
                    cards_len: 3,
                    primary: level_order_value(resolved[0], ctx),
                    bomb_tier: 0,
                })
            }
            5 => {
                if is_straight(&resolved)? {
                    let hi = straight_highest_natural(&resolved)?;
                    Ok(Combination {
                        kind: CombinationKind::Ordinary(OrdinaryKind::Straight),
                        cards_len: 5,
                        primary: hi,
                        bomb_tier: 0,
                    })
                } else if is_full_house(&resolved)? {
                    let triple_rank = full_house_triple_rank(&resolved)?;
                    Ok(Combination {
                        kind: CombinationKind::Ordinary(OrdinaryKind::FullHouse),
                        cards_len: 5,
                        primary: level_order_value(
                            Card {
                                suit: Suit::Spades,
                                rank: triple_rank,
                            },
                            ctx,
                        ),
                        bomb_tier: 0,
                    })
                } else {
                    Err("invalid 5-card ordinary combination".into())
                }
            }
            6 => {
                if is_tube(&resolved)? {
                    let hi = tube_highest_natural(&resolved)?;
                    Ok(Combination {
                        kind: CombinationKind::Ordinary(OrdinaryKind::Tube),
                        cards_len: 6,
                        primary: hi,
                        bomb_tier: 0,
                    })
                } else if is_plate(&resolved)? {
                    let hi = plate_highest_natural(&resolved)?;
                    Ok(Combination {
                        kind: CombinationKind::Ordinary(OrdinaryKind::Plate),
                        cards_len: 6,
                        primary: hi,
                        bomb_tier: 0,
                    })
                } else {
                    Err("invalid 6-card ordinary combination".into())
                }
            }
            _ => Err("unsupported combination length".into()),
        }
    }
}

fn all_same_rank(cards: &[Card]) -> bool {
    cards.iter().all(|c| c.rank == cards[0].rank)
}

fn ensure_same_rank(cards: &[Card]) -> Result<(), String> {
    if all_same_rank(cards) {
        Ok(())
    } else {
        Err("cards are not of the same rank".into())
    }
}

fn ensure_no_mixed_joker_pair(cards: &[Card]) -> Result<(), String> {
    if cards.len() != 2 {
        return Ok(());
    }
    let a = cards[0];
    let b = cards[1];
    if a.suit == Suit::Joker && b.suit == Suit::Joker && a.rank != b.rank {
        return Err("mixed joker pair is not allowed".into());
    }
    Ok(())
}

fn is_four_joker(cards: &[Card]) -> bool {
    if cards.len() != 4 {
        return false;
    }
    let mut red = 0;
    let mut black = 0;
    for c in cards {
        match c.rank {
            Rank::RedJoker => red += 1,
            Rank::BlackJoker => black += 1,
            _ => return false,
        }
    }
    red == 2 && black == 2
}

fn is_straight_flush(cards: &[Card]) -> Result<bool, String> {
    if cards.iter().any(|c| c.suit == Suit::Joker) {
        return Ok(false);
    }
    let suit = cards[0].suit;
    if suit == Suit::Joker || cards.iter().any(|c| c.suit != suit) {
        return Ok(false);
    }
    is_straight(cards)
}

fn is_full_house(cards: &[Card]) -> Result<bool, String> {
    if cards.len() != 5 {
        return Ok(false);
    }
    if cards.iter().any(|c| c.suit == Suit::Joker) {
        return Ok(false);
    }
    let counts = rank_counts(cards);
    if counts.len() != 2 {
        return Ok(false);
    }
    Ok(counts.values().any(|&n| n == 3) && counts.values().any(|&n| n == 2))
}

fn full_house_triple_rank(cards: &[Card]) -> Result<Rank, String> {
    let counts = rank_counts(cards);
    let (&rank, _) = counts
        .iter()
        .find(|&(_, &n)| n == 3)
        .ok_or_else(|| "not a full house".to_string())?;
    Ok(rank)
}

fn rank_counts(cards: &[Card]) -> HashMap<Rank, usize> {
    let mut m = HashMap::new();
    for c in cards {
        *m.entry(c.rank).or_insert(0) += 1;
    }
    m
}

fn is_straight(cards: &[Card]) -> Result<bool, String> {
    if cards.len() != 5 {
        return Ok(false);
    }
    if cards.iter().any(|c| c.suit == Suit::Joker) {
        return Ok(false);
    }
    // Straight must not be all same suit (otherwise it's straight flush bomb).
    let suit0 = cards[0].suit;
    if cards.iter().all(|c| c.suit == suit0) {
        return Ok(false);
    }
    Ok(straight_highest_natural(cards).is_ok())
}

fn straight_highest_natural(cards: &[Card]) -> Result<u8, String> {
    // Build multiset of natural ranks.
    let mut vals: Vec<u8> = cards
        .iter()
        .map(|c| natural_rank_value(c.rank))
        .collect::<Result<_, _>>()?;
    vals.sort_unstable();
    vals.dedup();
    if vals.len() != 5 {
        return Err("straight requires 5 distinct ranks".into());
    }

    // Try A-low: A,2,3,4,5 => treat A as 1.
    let is_a_low = vals == vec![2, 3, 4, 5, 14];
    if is_a_low {
        return Ok(5);
    }

    // Normal consecutive, with explicit prohibition of Ace interior forms like K-A-2-3-4.
    // With distinct sorted ranks, this reduces to consecutive check.
    for w in vals.windows(2) {
        if w[1] != w[0] + 1 {
            return Err("not consecutive in natural order".into());
        }
    }
    Ok(*vals.last().expect("len=5"))
}

fn is_tube(cards: &[Card]) -> Result<bool, String> {
    if cards.len() != 6 {
        return Ok(false);
    }
    if cards.iter().any(|c| c.suit == Suit::Joker) {
        return Ok(false);
    }
    let counts = rank_counts(cards);
    if counts.len() != 3 || counts.values().any(|&n| n != 2) {
        return Ok(false);
    }
    tube_highest_natural(cards).map(|_| true)
}

fn tube_highest_natural(cards: &[Card]) -> Result<u8, String> {
    let counts = rank_counts(cards);
    let mut vals: Vec<u8> = counts
        .keys()
        .map(|&r| natural_rank_value(r))
        .collect::<Result<_, _>>()?;
    vals.sort_unstable();
    // A-low tube: AA 22 33 => ranks {A,2,3}
    if vals == vec![2, 3, 14] {
        return Ok(3);
    }
    for w in vals.windows(2) {
        if w[1] != w[0] + 1 {
            return Err("tube pairs not consecutive".into());
        }
    }
    Ok(*vals.last().unwrap())
}

fn is_plate(cards: &[Card]) -> Result<bool, String> {
    if cards.len() != 6 {
        return Ok(false);
    }
    if cards.iter().any(|c| c.suit == Suit::Joker) {
        return Ok(false);
    }
    let counts = rank_counts(cards);
    if counts.len() != 2 || counts.values().any(|&n| n != 3) {
        return Ok(false);
    }
    plate_highest_natural(cards).map(|_| true)
}

fn plate_highest_natural(cards: &[Card]) -> Result<u8, String> {
    let counts = rank_counts(cards);
    let mut vals: Vec<u8> = counts
        .keys()
        .map(|&r| natural_rank_value(r))
        .collect::<Result<_, _>>()?;
    vals.sort_unstable();
    // A-low plate: AAA 222 => ranks {A,2}
    if vals == vec![2, 14] {
        return Ok(2);
    }
    if vals.len() != 2 || vals[1] != vals[0] + 1 {
        return Err("plate triples not consecutive".into());
    }
    Ok(vals[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::card::{HandLevel, RuleContext};

    #[test]
    fn straight_rejects_ace_interior() {
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let cards = vec![
            "♠K".to_string(),
            "♥A".to_string(),
            "♣2".to_string(),
            "♦3".to_string(),
            "♠4".to_string(),
        ];
        let err = CombinationParser::parse(&cards, None, ctx).unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn straight_accepts_a2345() {
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let cards = vec![
            "♠A".to_string(),
            "♦2".to_string(),
            "♣3".to_string(),
            "♦4".to_string(),
            "♠5".to_string(),
        ];
        let c = CombinationParser::parse(&cards, Some(&[]), ctx).unwrap();
        assert_eq!(c.kind, CombinationKind::Ordinary(OrdinaryKind::Straight));
        assert_eq!(c.primary, 5);
    }

    #[test]
    fn wild_targets_can_be_omitted_when_inferred_upstream() {
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let cards = vec!["♥2".to_string(), "♠2".to_string()];
        let err = CombinationParser::parse(&cards, None, ctx).unwrap_err();
        assert!(err.contains("wildTargets"));
    }

    #[test]
    fn pair_parses() {
        let ctx = RuleContext {
            hand_level: HandLevel::Ten,
        };
        let c = CombinationParser::parse(&["♠7".into(), "♥7".into()], None, ctx).unwrap();
        assert_eq!(c.kind, CombinationKind::Ordinary(OrdinaryKind::Pair));
    }

    #[test]
    fn triple_parses() {
        let ctx = RuleContext {
            hand_level: HandLevel::Ten,
        };
        let c =
            CombinationParser::parse(&["♠Q".into(), "♥Q".into(), "♦Q".into()], None, ctx).unwrap();
        assert_eq!(c.kind, CombinationKind::Ordinary(OrdinaryKind::Triple));
    }

    #[test]
    fn full_house_parses_triple_rank_primary() {
        let ctx = RuleContext {
            hand_level: HandLevel::Ten,
        };
        let c = CombinationParser::parse(
            &[
                "♠5".into(),
                "♥5".into(),
                "♦5".into(),
                "♣3".into(),
                "♠3".into(),
            ],
            None,
            ctx,
        )
        .unwrap();
        assert_eq!(c.kind, CombinationKind::Ordinary(OrdinaryKind::FullHouse));
    }

    #[test]
    fn quad_bomb_same_rank() {
        let ctx = RuleContext {
            hand_level: HandLevel::Ten,
        };
        let c = CombinationParser::parse(
            &["♠9".into(), "♥9".into(), "♦9".into(), "♣9".into()],
            None,
            ctx,
        )
        .unwrap();
        assert_eq!(c.kind, CombinationKind::Bomb(BombKind::SameRank { n: 4 }));
        assert_eq!(c.bomb_tier, 1);
    }

    #[test]
    fn wild_single_resolves_with_target() {
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let c = CombinationParser::parse(&["♥2".into()], Some(&["♠K".into()]), ctx).unwrap();
        assert_eq!(c.kind, CombinationKind::Ordinary(OrdinaryKind::Single));
    }
}
