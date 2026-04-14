//! Deterministic choice among [`super::enumerate_legal_actions`] for a single actor.

use std::cmp::Ordering;

use crate::domain::Seat;
use crate::game::card::{RuleContext, is_wild, level_order_value, parse_card_symbol};
use crate::game::engine::PlayerAction;
use crate::game::rules::combination_parser::{CombinationClass, CombinationParser};
use crate::game::types::{GamePhase, TableGameState};

use super::enumerate_legal_actions;

/// Pick one legal action:
/// - playing: non-bomb first, then smaller combination primary value, then more cards
/// - tribute/return: smaller card value first
pub fn suggest_next_action(state: &TableGameState, actor: Seat) -> Result<PlayerAction, String> {
    let legal = enumerate_legal_actions(state, actor)?;
    if legal.is_empty() {
        return Err("no legal actions".into());
    }

    let hand = state.hand.as_ref().ok_or_else(|| "no hand".to_string())?;
    let ctx = RuleContext {
        hand_level: hand.hand_level,
    };

    match state.phase {
        GamePhase::Playing => pick_playing(&legal, ctx),
        GamePhase::Tribute => pick_tribute(&legal, ctx),
        GamePhase::Exchange => pick_return(&legal, ctx),
        _ => Err("suggest_next_action: not in tribute, exchange, or playing".into()),
    }
}

fn pick_tribute(legal: &[PlayerAction], ctx: RuleContext) -> Result<PlayerAction, String> {
    let mut items: Vec<(u8, String, PlayerAction)> = Vec::new();
    for a in legal {
        let PlayerAction::Tribute { card } = a else {
            continue;
        };
        let c = parse_card_symbol(card)?;
        let v = level_order_value(c, ctx);
        items.push((v, card.clone(), a.clone()));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    items
        .into_iter()
        .next()
        .map(|(_, _, act)| act)
        .ok_or_else(|| "suggest: no tribute action".into())
}

fn pick_return(legal: &[PlayerAction], ctx: RuleContext) -> Result<PlayerAction, String> {
    let mut items: Vec<(u8, String, PlayerAction)> = Vec::new();
    for a in legal {
        let PlayerAction::ReturnCard { card } = a else {
            continue;
        };
        let c = parse_card_symbol(card)?;
        let v = level_order_value(c, ctx);
        items.push((v, card.clone(), a.clone()));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    items
        .into_iter()
        .next()
        .map(|(_, _, act)| act)
        .ok_or_else(|| "suggest: no return_card action".into())
}

fn pick_playing(legal: &[PlayerAction], ctx: RuleContext) -> Result<PlayerAction, String> {
    let plays: Vec<&PlayerAction> = legal
        .iter()
        .filter(|a| matches!(a, PlayerAction::Play { .. }))
        .collect();

    if plays.is_empty() {
        return legal
            .iter()
            .find(|a| matches!(a, PlayerAction::Pass))
            .cloned()
            .ok_or_else(|| "suggest: no pass in legal".into());
    }

    let mut best: Option<&PlayerAction> = None;
    for a in plays {
        if playing_cmp(a, best, ctx)? == Ordering::Less {
            best = Some(a);
        }
    }
    best.cloned()
        .ok_or_else(|| "suggest: empty play list".into())
}

/// Prefer `a` over `b` if Ordering::Less.
fn playing_cmp(
    a: &PlayerAction,
    b: Option<&PlayerAction>,
    ctx: RuleContext,
) -> Result<Ordering, String> {
    let Some(b) = b else {
        return Ok(Ordering::Less);
    };
    Ok(play_key(a, ctx)?.cmp(&play_key(b, ctx)?))
}

/// Sort key for preferred playing suggestion:
/// 1) fewer wildcard cards first (0 < 1 < 2 ...)
/// 2) non-bomb before bomb
/// 3) smaller combination primary value first
/// 4) if same primary, more cards first
/// 5) lexicographic card symbols for deterministic tie-break
fn play_key(
    a: &PlayerAction,
    ctx: RuleContext,
) -> Result<(usize, bool, u8, std::cmp::Reverse<usize>, Vec<String>), String> {
    match a {
        PlayerAction::Play {
            cards,
            wild_targets,
        } => {
            let combo = CombinationParser::parse(cards, wild_targets.as_deref(), ctx)?;
            let wild_count = cards.iter().try_fold(0usize, |acc, s| {
                let c = parse_card_symbol(s)?;
                Ok::<usize, String>(acc + usize::from(is_wild(c, ctx)))
            })?;
            let is_bomb = matches!(combo.class(), CombinationClass::Bomb);
            let mut sorted = cards.clone();
            sorted.sort();
            Ok((
                wild_count,
                is_bomb,
                combo.primary,
                std::cmp::Reverse(cards.len()),
                sorted,
            ))
        }
        _ => Err("suggest: play_key expects Play action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::card::HandLevel;
    use crate::game::rules::combination_parser::CombinationParser;
    use crate::game::types::{HandState, PlayState};

    fn ctx() -> RuleContext {
        RuleContext {
            hand_level: HandLevel::Two,
        }
    }

    fn mk_playing_state(
        actor: Seat,
        actor_hand: Vec<&str>,
        top_cards: Option<(Seat, Vec<&str>)>,
    ) -> TableGameState {
        let mut s = TableGameState::new("t_suggest".into());
        s.phase = GamePhase::Playing;
        s.turn_seat = actor;
        s.leader_seat = actor;

        let mut hand = HandState::new(HandLevel::Two);
        hand.hands.insert(
            actor,
            actor_hand.into_iter().map(ToString::to_string).collect(),
        );
        for seat in Seat::ALL {
            hand.hands.entry(seat).or_insert_with(Vec::new);
        }

        if let Some((seat, cards)) = top_cards {
            let cards: Vec<String> = cards.into_iter().map(ToString::to_string).collect();
            let combo = CombinationParser::parse(&cards, None, ctx()).unwrap();
            hand.trick.top_play = Some(PlayState {
                seat,
                cards: cards.clone(),
                wild_targets: None,
                combination: combo,
            });
            hand.trick.last_play_seat = Some(seat);
        }

        s.hand = Some(hand);
        s
    }

    #[test]
    fn prefers_non_bomb_over_bomb() {
        let legal = vec![
            PlayerAction::Play {
                cards: vec!["♠3".into()],
                wild_targets: None,
            },
            PlayerAction::Play {
                cards: vec!["♠4".into(), "♥4".into(), "♦4".into(), "♣4".into()],
                wild_targets: None,
            },
        ];
        let picked = pick_playing(&legal, ctx()).unwrap();
        assert_eq!(
            picked,
            PlayerAction::Play {
                cards: vec!["♠3".into()],
                wild_targets: None,
            }
        );
    }

    #[test]
    fn prefers_smaller_primary_value() {
        let legal = vec![
            PlayerAction::Play {
                cards: vec!["♠7".into()],
                wild_targets: None,
            },
            PlayerAction::Play {
                cards: vec!["♠9".into()],
                wild_targets: None,
            },
        ];
        let picked = pick_playing(&legal, ctx()).unwrap();
        assert_eq!(
            picked,
            PlayerAction::Play {
                cards: vec!["♠7".into()],
                wild_targets: None,
            }
        );
    }

    #[test]
    fn prefers_more_cards_when_primary_is_same() {
        let legal = vec![
            PlayerAction::Play {
                cards: vec!["♠7".into()],
                wild_targets: None,
            },
            PlayerAction::Play {
                cards: vec!["♠7".into(), "♥7".into()],
                wild_targets: None,
            },
        ];
        let picked = pick_playing(&legal, ctx()).unwrap();
        assert_eq!(
            picked,
            PlayerAction::Play {
                cards: vec!["♠7".into(), "♥7".into()],
                wild_targets: None,
            }
        );
    }

    #[test]
    fn suggest_follow_play_prefers_non_bomb_when_both_legal() {
        let state = mk_playing_state(
            Seat::E,
            vec!["♠7", "♠8", "♥8", "♦8", "♣8"],
            Some((Seat::N, vec!["♠6"])),
        );
        let picked = suggest_next_action(&state, Seat::E).unwrap();
        assert_eq!(
            picked,
            PlayerAction::Play {
                cards: vec!["♠7".into()],
                wild_targets: None,
            }
        );
    }

    #[test]
    fn suggest_returns_pass_when_no_play_can_beat_top() {
        let state = mk_playing_state(Seat::E, vec!["♠3"], Some((Seat::N, vec!["♠A"])));
        let picked = suggest_next_action(&state, Seat::E).unwrap();
        assert_eq!(picked, PlayerAction::Pass);
    }

    #[test]
    fn prefers_fewer_wildcards_before_other_priorities() {
        // Hand level 2, so all "2" cards are wildcards.
        // Both plays can beat top single "♠6", and both are non-bomb singles.
        // Expect non-wild "♠7" to be preferred over wild "♠2".
        let state = mk_playing_state(Seat::E, vec!["♠2", "♠7"], Some((Seat::N, vec!["♠6"])));
        let picked = suggest_next_action(&state, Seat::E).unwrap();
        assert_eq!(
            picked,
            PlayerAction::Play {
                cards: vec!["♠7".into()],
                wild_targets: None,
            }
        );
    }
}
