//! Enumerate legal [`PlayerAction`](crate::game::engine::PlayerAction)s for the current actor.

use std::collections::HashSet;

use crate::domain::Seat;
use crate::game::card::{is_wild, level_order_value, parse_card_symbol, RuleContext};
use crate::game::engine::PlayerAction;
use crate::game::rules::beat_comparator::BeatComparator;
use crate::game::rules::combination_parser::CombinationParser;
use crate::game::types::{GamePhase, HandState, TableGameState};

/// Hard cap on combination-index iterations per `enumerate_legal_actions` call.
const MAX_COMBO_TRIES: usize = 120_000;

/// Max Cartesian products for wildcard target enumeration per card subset.
const MAX_WILD_PRODUCT: usize = 128;

/// All 52 suit/rank symbols (for wildcard target enumeration).
fn all_non_joker_symbols() -> Vec<String> {
    let suits = ["♠", "♥", "♦", "♣"];
    let ranks = [
        "A", "K", "Q", "J", "10", "9", "8", "7", "6", "5", "4", "3", "2",
    ];
    let mut v = Vec::with_capacity(52);
    for s in suits {
        for r in ranks {
            v.push(format!("{}{}", s, r));
        }
    }
    v
}

/// Candidate symbols that may appear as wildcard targets (bounded).
fn wild_target_pool(hand: &[String], ctx: RuleContext) -> Vec<String> {
    let mut set: HashSet<String> = all_non_joker_symbols().into_iter().collect();
    for s in hand {
        if let Ok(c) = parse_card_symbol(s) {
            if !is_wild(c, ctx) {
                set.insert(s.clone());
            }
        }
    }
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort();
    v
}

fn combinations_of_indices(n: usize, k: usize, f: &mut impl FnMut(&[usize]) -> bool) {
    if k == 0 || k > n {
        return;
    }
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        if !f(&idx) {
            return;
        }
        let mut i = k;
        while i > 0 && idx[i - 1] == n - k + i - 1 {
            i -= 1;
        }
        if i == 0 {
            return;
        }
        i -= 1;
        idx[i] += 1;
        for j in i + 1..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
}

fn wild_positions_in_cards(cards: &[String], ctx: RuleContext) -> Vec<usize> {
    cards
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            parse_card_symbol(s)
                .ok()
                .filter(|&c| is_wild(c, ctx))
                .map(|_| i)
        })
        .collect()
}

fn try_play(
    cards: &[String],
    wild_targets: Option<&[String]>,
    ctx: RuleContext,
    top: Option<&crate::game::rules::combination_parser::Combination>,
) -> Option<crate::game::rules::combination_parser::Combination> {
    let combo = CombinationParser::parse(cards, wild_targets, ctx).ok()?;
    if let Some(t) = top {
        if !BeatComparator::can_beat(t, &combo) {
            return None;
        }
    }
    Some(combo)
}

fn push_play_unique(
    out: &mut Vec<PlayerAction>,
    seen: &mut HashSet<(Vec<String>, Option<Vec<String>>)>,
    cards: Vec<String>,
    wild_targets: Option<Vec<String>>,
    ctx: RuleContext,
    top: Option<&crate::game::rules::combination_parser::Combination>,
) {
    let wt = wild_targets.clone();
    if try_play(&cards, wt.as_deref(), ctx, top).is_none() {
        return;
    }
    let key = (cards.clone(), wild_targets.clone());
    if seen.insert(key) {
        out.push(PlayerAction::Play {
            cards,
            wild_targets,
        });
    }
}

fn enumerate_wild_products(
    pool: &[String],
    wild_count: usize,
    mut f: impl FnMut(&[String]) -> bool,
) {
    if wild_count == 0 {
        let empty: [String; 0] = [];
        f(&empty);
        return;
    }
    let mut buf = vec![pool.first().cloned().unwrap_or_default(); wild_count];
    let mut count = 0usize;
    fn rec(
        pool: &[String],
        buf: &mut [String],
        pos: usize,
        count: &mut usize,
        max: usize,
        f: &mut impl FnMut(&[String]) -> bool,
    ) -> bool {
        if *count >= max {
            return false;
        }
        if pos == buf.len() {
            *count += 1;
            return f(buf);
        }
        for t in pool {
            buf[pos] = t.clone();
            if !rec(pool, buf, pos + 1, count, max, f) {
                return false;
            }
        }
        true
    }
    let mut fmut = f;
    rec(pool, &mut buf, 0, &mut count, MAX_WILD_PRODUCT, &mut fmut);
}

fn enumerate_playing(
    hand_state: &HandState,
    actor: Seat,
    ctx: RuleContext,
) -> Result<Vec<PlayerAction>, String> {
    let h = hand_state
        .hands
        .get(&actor)
        .ok_or_else(|| "missing actor hand".to_string())?;
    let top = hand_state
        .trick
        .top_play
        .as_ref()
        .map(|p| &p.combination);

    // No cards: must pass (engine).
    if h.is_empty() {
        return Ok(vec![PlayerAction::Pass]);
    }

    let mut out = Vec::new();
    let mut seen: HashSet<(Vec<String>, Option<Vec<String>>)> = HashSet::new();
    let n = h.len();
    let max_k = n.min(10);

    // Leading: cannot pass while holding cards.
    let may_pass = top.is_some();

    if may_pass {
        out.push(PlayerAction::Pass);
        seen.insert((vec![], None)); // not used for Pass dedup
    }

    let pool = wild_target_pool(h, ctx);
    let mut tries = 0usize;

    for k in 1..=max_k {
        combinations_of_indices(n, k, &mut |idxs| {
            if tries >= MAX_COMBO_TRIES {
                return false;
            }
            tries += 1;
            let cards: Vec<String> = idxs.iter().map(|&i| h[i].clone()).collect();
            let wilds = wild_positions_in_cards(&cards, ctx);
            if wilds.is_empty() {
                push_play_unique(&mut out, &mut seen, cards, None, ctx, top);
            } else {
                let wn = wilds.len();
                enumerate_wild_products(&pool, wn, |targets| {
                    push_play_unique(
                        &mut out,
                        &mut seen,
                        cards.clone(),
                        Some(targets.to_vec()),
                        ctx,
                        top,
                    );
                    true
                });
            }
            true
        });
        if tries >= MAX_COMBO_TRIES {
            break;
        }
    }

    // If leading and nothing parsed, fail fast (should not happen if singles exist).
    if top.is_none() && !h.is_empty() {
        let has_play = out.iter().any(|a| matches!(a, PlayerAction::Play { .. }));
        if !has_play {
            return Err("movegen: no legal lead play found (budget or rules)".into());
        }
    }

    Ok(out)
}

fn enumerate_tribute(hand_state: &HandState, actor: Seat, ctx: RuleContext) -> Vec<PlayerAction> {
    let h = match hand_state.hands.get(&actor) {
        Some(x) => x,
        None => return vec![],
    };
    let mut best = 0u8;
    for s in h {
        if let Ok(c) = parse_card_symbol(s) {
            if !is_wild(c, ctx) {
                best = best.max(level_order_value(c, ctx));
            }
        }
    }
    let mut out = Vec::new();
    for s in h {
        if let Ok(c) = parse_card_symbol(s) {
            if !is_wild(c, ctx) && level_order_value(c, ctx) == best {
                out.push(PlayerAction::Tribute {
                    card: s.clone(),
                });
            }
        }
    }
    out
}

fn enumerate_return(hand_state: &HandState, actor: Seat) -> Result<Vec<PlayerAction>, String> {
    let tribute = hand_state
        .tribute
        .as_ref()
        .ok_or_else(|| "missing tribute".to_string())?;
    let pair = tribute
        .pairs
        .iter()
        .find(|p| p.receiver == actor && p.return_card.is_none())
        .ok_or_else(|| "not return actor".to_string())?;
    let paid = pair
        .paid_card
        .as_ref()
        .ok_or_else(|| "tribute not paid".to_string())?;
    let paid_rank = parse_card_symbol(paid)?.rank;
    let h = hand_state
        .hands
        .get(&actor)
        .ok_or_else(|| "missing hand".to_string())?;
    let mut out = Vec::new();
    for s in h {
        let r = parse_card_symbol(s)?.rank;
        if r != paid_rank {
            out.push(PlayerAction::ReturnCard { card: s.clone() });
        }
    }
    Ok(out)
}

/// Seat that must act next (differs from [`TableGameState::turn_seat`] during tribute / exchange).
pub fn current_actor_seat(state: &TableGameState) -> Option<Seat> {
    let h = state.hand.as_ref()?;
    match state.phase {
        GamePhase::Tribute => {
            if let Some(a) = h.next_tribute_actor() {
                return Some(a);
            }
            let t = h.tribute.as_ref()?;
            if t.canceled {
                return t.opening_lead_candidates.first().copied();
            }
            None
        }
        GamePhase::Exchange => h.next_exchange_actor(),
        GamePhase::Playing => Some(state.turn_seat),
        GamePhase::Scoring
        | GamePhase::Dealing
        | GamePhase::TableSetup
        | GamePhase::Completed => None,
    }
}

/// All legal actions for `actor` when they are the current actor ([`current_actor_seat`]).
pub fn enumerate_legal_actions(state: &TableGameState, actor: Seat) -> Result<Vec<PlayerAction>, String> {
    if current_actor_seat(state) != Some(actor) {
        return Err("not actor turn".into());
    }
    let hand = state
        .hand
        .as_ref()
        .ok_or_else(|| "no hand".to_string())?;
    let ctx = RuleContext {
        hand_level: hand.hand_level,
    };

    match state.phase {
        GamePhase::Playing => enumerate_playing(hand, actor, ctx),
        GamePhase::Tribute => Ok(enumerate_tribute(hand, actor, ctx)),
        GamePhase::Exchange => enumerate_return(hand, actor),
        GamePhase::Scoring
        | GamePhase::Dealing
        | GamePhase::TableSetup
        | GamePhase::Completed => Ok(vec![]),
    }
}
