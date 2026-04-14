use std::collections::HashSet;

use crate::game::card::{
    Card, Rank, RuleContext, Suit, is_wild, natural_rank_value, parse_card_symbol, to_card_symbol,
};
use crate::game::rules::beat_comparator::BeatComparator;
use crate::game::rules::combination_parser::{
    BombKind, Combination, CombinationClass, CombinationKind, CombinationParser, OrdinaryKind,
};

pub struct WildTargetInfer;

impl WildTargetInfer {
    pub fn infer_best(
        cards: &[String],
        ctx: RuleContext,
        top: Option<&Combination>,
    ) -> Result<Option<Vec<String>>, String> {
        let parsed: Vec<Card> = cards
            .iter()
            .map(|s| parse_card_symbol(s))
            .collect::<Result<_, _>>()?;
        let wild_count = parsed.iter().filter(|c| is_wild(**c, ctx)).count();
        if wild_count == 0 {
            return Ok(None);
        }

        let pruned_pool = pruned_pool(&parsed, ctx);
        let mut best = search_best(cards, ctx, top, &pruned_pool);
        if best.is_none() {
            let full_pool = all_non_joker_symbols();
            best = search_best(cards, ctx, top, &full_pool);
        }
        Ok(best)
    }
}

fn search_best(
    cards: &[String],
    ctx: RuleContext,
    top: Option<&Combination>,
    pool: &[String],
) -> Option<Vec<String>> {
    let wild_count = cards
        .iter()
        .filter_map(|s| parse_card_symbol(s).ok())
        .filter(|c| is_wild(*c, ctx))
        .count();
    if wild_count == 0 {
        return None;
    }
    if pool.is_empty() {
        return None;
    }

    let mut cur = vec![String::new(); wild_count];
    let mut best: Option<(StrengthKey, Vec<String>)> = None;

    fn rec(
        pos: usize,
        cur: &mut [String],
        cards: &[String],
        ctx: RuleContext,
        top: Option<&Combination>,
        pool: &[String],
        best: &mut Option<(StrengthKey, Vec<String>)>,
    ) {
        if pos == cur.len() {
            let combo = match CombinationParser::parse(cards, Some(cur), ctx) {
                Ok(c) => c,
                Err(_) => return,
            };
            if let Some(t) = top
                && !BeatComparator::can_beat(t, &combo)
            {
                return;
            }
            let key = strength_key(&combo, cur);
            match best {
                Some((bk, _)) if key <= *bk => {}
                _ => {
                    *best = Some((key, cur.to_vec()));
                }
            }
            return;
        }
        for t in pool {
            cur[pos] = t.clone();
            rec(pos + 1, cur, cards, ctx, top, pool, best);
        }
    }

    rec(0, &mut cur, cards, ctx, top, pool, &mut best);
    best.map(|(_, targets)| targets)
}

fn pruned_pool(parsed_cards: &[Card], ctx: RuleContext) -> Vec<String> {
    let mut ranks = HashSet::new();
    let non_wild: Vec<Card> = parsed_cards
        .iter()
        .copied()
        .filter(|c| !is_wild(*c, ctx) && c.suit != Suit::Joker)
        .collect();

    for c in &non_wild {
        ranks.insert(c.rank);
        for r in adjacent_ranks(c.rank) {
            ranks.insert(r);
        }
    }
    ranks.insert(ctx.hand_level.to_rank());

    if ranks.is_empty() {
        ranks.insert(ctx.hand_level.to_rank());
    }

    let suits = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];
    let mut out = Vec::new();
    for r in ranks {
        for s in suits {
            out.push(to_card_symbol(Card { suit: s, rank: r }));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn adjacent_ranks(rank: Rank) -> Vec<Rank> {
    let Ok(v) = natural_rank_value(rank) else {
        return vec![];
    };
    let mut out = Vec::new();
    if v > 2
        && let Some(r) = rank_from_natural(v - 1)
    {
        out.push(r);
    }
    if v < 14
        && let Some(r) = rank_from_natural(v + 1)
    {
        out.push(r);
    }
    // Keep A-low adjacency for sequence-family heuristics.
    if v == 14 {
        out.push(Rank::Two);
    }
    if v == 2 {
        out.push(Rank::A);
    }
    out
}

fn rank_from_natural(v: u8) -> Option<Rank> {
    Some(match v {
        2 => Rank::Two,
        3 => Rank::Three,
        4 => Rank::Four,
        5 => Rank::Five,
        6 => Rank::Six,
        7 => Rank::Seven,
        8 => Rank::Eight,
        9 => Rank::Nine,
        10 => Rank::Ten,
        11 => Rank::J,
        12 => Rank::Q,
        13 => Rank::K,
        14 => Rank::A,
        _ => return None,
    })
}

fn all_non_joker_symbols() -> Vec<String> {
    let suits = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];
    let ranks = [
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
    ];
    let mut v = Vec::with_capacity(52);
    for s in suits {
        for r in ranks {
            v.push(to_card_symbol(Card { suit: s, rank: r }));
        }
    }
    v
}

type StrengthKey = (u8, u8, u8, u8, Vec<String>);

fn strength_key(combo: &Combination, wild_targets: &[String]) -> StrengthKey {
    let class_rank = match combo.class() {
        CombinationClass::Ordinary => 0,
        CombinationClass::Bomb => 1,
    };
    let kind_rank = kind_rank(combo.kind);
    (
        class_rank,
        combo.bomb_tier,
        combo.primary,
        kind_rank,
        wild_targets.to_vec(),
    )
}

fn kind_rank(kind: CombinationKind) -> u8 {
    match kind {
        CombinationKind::Ordinary(OrdinaryKind::Single) => 1,
        CombinationKind::Ordinary(OrdinaryKind::Pair) => 2,
        CombinationKind::Ordinary(OrdinaryKind::Triple) => 3,
        CombinationKind::Ordinary(OrdinaryKind::Straight) => 4,
        CombinationKind::Ordinary(OrdinaryKind::Tube) => 5,
        CombinationKind::Ordinary(OrdinaryKind::Plate) => 6,
        CombinationKind::Ordinary(OrdinaryKind::FullHouse) => 7,
        CombinationKind::Bomb(BombKind::SameRank { n }) => 10 + n,
        CombinationKind::Bomb(BombKind::StraightFlush) => 30,
        CombinationKind::Bomb(BombKind::FourJoker) => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::card::HandLevel;
    use crate::game::rules::combination_parser::CombinationParser;

    #[test]
    fn infer_none_for_non_wild_play() {
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let out = WildTargetInfer::infer_best(&["♠A".into()], ctx, None).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn infer_prefers_bomb_when_leading() {
        let ctx = RuleContext {
            hand_level: HandLevel::Two,
        };
        let cards = vec!["♥2".into(), "♠A".into(), "♥A".into(), "♦A".into()];
        let inferred = WildTargetInfer::infer_best(&cards, ctx, None)
            .unwrap()
            .unwrap();
        let combo = CombinationParser::parse(&cards, Some(&inferred), ctx).unwrap();
        assert!(matches!(combo.kind, CombinationKind::Bomb(_)));
    }
}
