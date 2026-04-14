use crate::domain::Seat;
use crate::game::card::{
    HandLevel, RuleContext, is_wild, level_order_value, parse_card_symbol, sort_card_symbols_desc,
    to_card_symbol,
};
use crate::game::deck::Deck;
use crate::game::rules::beat_comparator::BeatComparator;
use crate::game::rules::combination_parser::{CombinationParser, combination_kind_api_type};
use crate::game::rules::wild_target_infer::WildTargetInfer;
use crate::game::types::{
    GameConfig, GamePhase, HandCommitMeta, HandHistoryEntry, HandState, HistoryActionKind,
    TableGameState, TeamId, TributePair, TributeState,
};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    Tribute {
        card: String,
    },
    ReturnCard {
        card: String,
    },
    Play {
        cards: Vec<String>,
        wild_targets: Option<Vec<String>>,
    },
    Pass,
}

impl PlayerAction {
    /// Payload shape for [`crate::store::parse_player_action`] / HTTP `actions/*` bodies (snake_case keys).
    pub fn to_store_payload(&self) -> (&'static str, serde_json::Value) {
        match self {
            PlayerAction::Tribute { card } => ("tribute", json!({ "card": card })),
            PlayerAction::ReturnCard { card } => ("return_card", json!({ "card": card })),
            PlayerAction::Play {
                cards,
                wild_targets: None,
            } => ("play", json!({ "cards": cards })),
            PlayerAction::Play {
                cards,
                wild_targets: Some(wt),
            } => (
                "play",
                json!({
                    "cards": cards,
                    "declaredWildMapping": { "wildTargets": wt }
                }),
            ),
            PlayerAction::Pass => ("pass", json!({})),
        }
    }

    /// Path suffix under `actions/<suffix>` and JSON body for HTTP POST (camelCase `playerId` / `seq`).
    pub fn to_http_action_request(
        &self,
        player_id: &str,
        seq: u64,
    ) -> (&'static str, serde_json::Value) {
        match self {
            PlayerAction::Tribute { card } => (
                "tribute",
                json!({ "playerId": player_id, "seq": seq, "card": card }),
            ),
            PlayerAction::ReturnCard { card } => (
                "return_card",
                json!({ "playerId": player_id, "seq": seq, "card": card }),
            ),
            PlayerAction::Play {
                cards,
                wild_targets: None,
            } => (
                "play",
                json!({
                    "playerId": player_id,
                    "seq": seq,
                    "cards": cards,
                }),
            ),
            PlayerAction::Play {
                cards,
                wild_targets: Some(wt),
            } => {
                let mut v = json!({
                    "playerId": player_id,
                    "seq": seq,
                    "cards": cards,
                });
                v["declaredWildMapping"] = json!({ "wildTargets": wt });
                ("play", v)
            }
            PlayerAction::Pass => ("pass", json!({ "playerId": player_id, "seq": seq })),
        }
    }

    /// Inverse of [`Self::to_store_payload`] for suggest API responses and tests.
    pub fn try_from_action_type_payload(
        action_type: &str,
        payload: &serde_json::Value,
    ) -> Result<Self, String> {
        match action_type {
            "tribute" => {
                let card = payload
                    .get("card")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing card".to_string())?
                    .to_string();
                Ok(PlayerAction::Tribute { card })
            }
            "return_card" => {
                let card = payload
                    .get("card")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing card".to_string())?
                    .to_string();
                Ok(PlayerAction::ReturnCard { card })
            }
            "play" => {
                let cards: Vec<String> = serde_json::from_value(
                    payload
                        .get("cards")
                        .cloned()
                        .ok_or_else(|| "missing cards".to_string())?,
                )
                .map_err(|e| format!("cards: {}", e))?;
                let wild_targets = payload
                    .get("declaredWildMapping")
                    .and_then(|v| v.get("wildTargets"))
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());
                Ok(PlayerAction::Play {
                    cards,
                    wild_targets,
                })
            }
            "pass" => Ok(PlayerAction::Pass),
            _ => Err(format!("unknown action_type {:?}", action_type)),
        }
    }
}

pub struct GameEngine {
    pub cfg: GameConfig,
}

impl GameEngine {
    pub fn new(cfg: GameConfig) -> Self {
        Self { cfg }
    }

    pub fn init_table(&self, table_id: String) -> TableGameState {
        TableGameState::new(table_id)
    }

    pub fn apply_player_action(
        &self,
        state: &mut TableGameState,
        actor_seat: Seat,
        action: PlayerAction,
        playing_commit: Option<HandCommitMeta>,
    ) -> Result<(), String> {
        match state.phase {
            GamePhase::Tribute => self.apply_tribute(state, actor_seat, action),
            GamePhase::Exchange => self.apply_exchange(state, actor_seat, action),
            GamePhase::Playing => {
                self.apply_playing(state, actor_seat, action, playing_commit.as_ref())
            }
            _ => Err("action not allowed in current phase".into()),
        }
    }

    pub fn start_first_hand(
        &self,
        state: &mut TableGameState,
        first_drawer: Seat,
        hand_level: HandLevel,
    ) -> Result<(), String> {
        state.hand_index += 1;
        state.phase = GamePhase::Dealing;
        let mut hand = HandState::new(hand_level);
        let deck = Deck::new_shuffled_double_deck(self.cfg.rng_seed + state.hand_index as u64);
        let dealt = deck.deal_27_each_ccw_from(first_drawer);
        for s in Seat::ALL {
            let cards = dealt
                .get(&s)
                .ok_or_else(|| "dealing missing seat".to_string())?
                .iter()
                .map(|c| to_card_symbol(*c))
                .collect();
            hand.hands.insert(s, cards);
        }
        state.hand = Some(hand);
        state.leader_seat = first_drawer;
        state.turn_seat = first_drawer;
        state.phase = GamePhase::Playing;
        Ok(())
    }

    pub fn start_next_hand_with_tribute(
        &self,
        state: &mut TableGameState,
        declarer: TeamId,
        hand_level: HandLevel,
        last_finishing_order: &[Seat],
    ) -> Result<(), String> {
        state.hand_index += 1;
        state.phase = GamePhase::Dealing;
        let mut hand = HandState::new(hand_level);
        let deck = Deck::new_shuffled_double_deck(self.cfg.rng_seed + state.hand_index as u64);
        let first_drawer = *last_finishing_order
            .last()
            .ok_or_else(|| "missing finishing order".to_string())?;
        let dealt = deck.deal_27_each_ccw_from(first_drawer);
        for s in Seat::ALL {
            let cards = dealt
                .get(&s)
                .ok_or_else(|| "dealing missing seat".to_string())?
                .iter()
                .map(|c| to_card_symbol(*c))
                .collect();
            hand.hands.insert(s, cards);
        }
        let tribute = build_tribute_plan(&hand, declarer, last_finishing_order)?;
        apply_initial_phase_from_tribute_plan(state, &tribute)?;
        hand.tribute = Some(tribute);
        state.hand = Some(hand);
        Ok(())
    }
}

impl GameEngine {
    fn apply_tribute(
        &self,
        state: &mut TableGameState,
        actor: Seat,
        action: PlayerAction,
    ) -> Result<(), String> {
        let hand = state
            .hand
            .as_mut()
            .ok_or_else(|| "missing hand".to_string())?;
        ensure_tribute_phase_ready(hand)?;
        let tribute = hand
            .tribute
            .as_ref()
            .ok_or_else(|| "missing tribute plan".to_string())?;
        if tribute.canceled {
            state.phase = GamePhase::Playing;
            if let Some(s) = tribute.opening_lead_candidates.first().copied() {
                state.turn_seat = s;
                state.leader_seat = s;
            }
            return Ok(());
        }
        let PlayerAction::Tribute { card } = action else {
            return Err("only tribute action allowed".into());
        };
        ensure_is_highest_non_wild(hand, actor, &card)?;
        let tribute = hand
            .tribute
            .as_mut()
            .ok_or_else(|| "missing tribute plan".to_string())?;
        let pair = tribute
            .pairs
            .iter_mut()
            .find(|p| p.payer == actor && p.paid_card.is_none())
            .ok_or_else(|| "player is not expected to tribute".to_string())?;
        remove_cards_from_hand(&mut hand.hands, actor, std::slice::from_ref(&card))?;
        pair.paid_card = Some(card);

        if tribute.pairs.iter().all(|p| p.paid_card.is_some()) {
            state.phase = GamePhase::Exchange;
        }
        Ok(())
    }

    fn apply_exchange(
        &self,
        state: &mut TableGameState,
        actor: Seat,
        action: PlayerAction,
    ) -> Result<(), String> {
        let hand = state
            .hand
            .as_mut()
            .ok_or_else(|| "missing hand".to_string())?;
        let tribute = hand
            .tribute
            .as_mut()
            .ok_or_else(|| "missing tribute plan".to_string())?;
        if tribute.canceled {
            state.phase = GamePhase::Playing;
            return Ok(());
        }
        let PlayerAction::ReturnCard { card } = action else {
            return Err("only return_card action allowed".into());
        };
        let pair = tribute
            .pairs
            .iter_mut()
            .find(|p| p.receiver == actor && p.return_card.is_none())
            .ok_or_else(|| "player is not expected to return card".to_string())?;
        let paid = pair
            .paid_card
            .clone()
            .ok_or_else(|| "tribute not yet paid".to_string())?;
        let paid_rank = parse_card_symbol(&paid)?.rank;
        let return_rank = parse_card_symbol(&card)?.rank;
        if paid_rank == return_rank {
            return Err("return card must have different rank than tribute card".into());
        }
        remove_cards_from_hand(&mut hand.hands, actor, std::slice::from_ref(&card))?;
        pair.return_card = Some(card.clone());
        // exchange: receiver gives card to payer
        hand.hands.get_mut(&pair.payer).expect("payer").push(card);
        // payer gives tribute card to receiver
        hand.hands
            .get_mut(&pair.receiver)
            .expect("receiver")
            .push(paid);

        if tribute.pairs.iter().all(|p| p.return_card.is_some()) {
            state.phase = GamePhase::Playing;
            let lead = tie_break_opening_leader(
                tribute.opening_lead_candidates[0],
                &tribute.opening_lead_candidates,
            );
            state.turn_seat = lead;
            state.leader_seat = lead;
        }
        Ok(())
    }

    fn apply_playing(
        &self,
        state: &mut TableGameState,
        actor: Seat,
        action: PlayerAction,
        commit: Option<&HandCommitMeta>,
    ) -> Result<(), String> {
        if state.turn_seat != actor {
            return Err("wrong turn".into());
        }
        let hand = state
            .hand
            .as_mut()
            .ok_or_else(|| "missing hand".to_string())?;
        let ctx = RuleContext {
            hand_level: hand.hand_level,
        };

        // If actor has no cards left, only pass is allowed.
        if hand.remaining_count(actor) == 0 {
            if !matches!(action, PlayerAction::Pass) {
                return Err("player has no cards left; must pass".into());
            }
        }

        match action {
            PlayerAction::Play {
                cards,
                wild_targets,
            } => {
                if cards.is_empty() {
                    return Err("empty play".into());
                }
                // If no top play, leader cannot pass; play is OK.
                let top = hand.trick.top_play.as_ref().map(|p| &p.combination);

                // Verify ownership before any mutation.
                ensure_cards_in_hand_multiset(&hand.hands, actor, &cards)?;

                let final_wild_targets = if wild_targets.is_some() {
                    wild_targets.clone()
                } else {
                    WildTargetInfer::infer_best(&cards, ctx, top)?.or_else(|| wild_targets.clone())
                };
                let combo = CombinationParser::parse(&cards, final_wild_targets.as_deref(), ctx)?;
                if let Some(topc) = top {
                    if !BeatComparator::can_beat(topc, &combo) {
                        return Err("play does not beat current top".into());
                    }
                }

                // Mutation happens only after all checks pass.
                remove_cards_from_hand(&mut hand.hands, actor, &cards)?;
                let mut sorted_cards = cards.clone();
                sort_card_symbols_desc(&mut sorted_cards, hand.hand_level);

                // Record top play.
                hand.trick.top_play = Some(crate::game::types::PlayState {
                    seat: actor,
                    cards: sorted_cards.clone(),
                    wild_targets: final_wild_targets.clone(),
                    combination: combo.clone(),
                });
                hand.trick.consecutive_passes = 0;
                hand.trick.last_play_seat = Some(actor);

                // Update finishing order.
                if hand.remaining_count(actor) == 0 && !hand.finishing_order.contains(&actor) {
                    hand.finishing_order.push(actor);
                }

                // Next turn skips seats that already ran out of cards.
                state.turn_seat = next_active_ccw(hand, actor);

                if let Some(meta) = commit {
                    hand.history.push(HandHistoryEntry {
                        seq: meta.seq,
                        action_id: format!("a_{}", meta.seq),
                        seat: actor,
                        timestamp: meta.timestamp.clone(),
                        action_type: HistoryActionKind::Play,
                        cards: sorted_cards.clone(),
                        combination_type: Some(combination_kind_api_type(&combo.kind)),
                        wild_targets: final_wild_targets.clone(),
                    });
                }
            }
            PlayerAction::Pass => {
                if hand.trick.top_play.is_none() {
                    // Leading cannot pass unless has no cards (handled above) — but if hand is empty,
                    // this branch may still occur, allow it and redirect leader.
                    if hand.remaining_count(actor) > 0 {
                        return Err("cannot pass when leading a trick".into());
                    }
                }

                hand.trick.consecutive_passes = hand.trick.consecutive_passes.saturating_add(1);
                state.turn_seat = next_active_ccw(hand, actor);

                let required_passes = passes_required_to_end_trick(hand, hand.trick.last_play_seat);
                if hand.trick.consecutive_passes >= required_passes {
                    // Trick ends; last successful play leads next trick.
                    let lead = hand
                        .trick
                        .last_play_seat
                        .ok_or_else(|| "trick ended without a play".to_string())?;
                    hand.trick.top_play = None;
                    hand.trick.consecutive_passes = 0;
                    state.turn_seat = lead;
                    state.leader_seat = lead;
                }

                if let Some(meta) = commit {
                    hand.history.push(HandHistoryEntry {
                        seq: meta.seq,
                        action_id: format!("a_{}", meta.seq),
                        seat: actor,
                        timestamp: meta.timestamp.clone(),
                        action_type: HistoryActionKind::Pass,
                        cards: Vec::new(),
                        combination_type: None,
                        wild_targets: None,
                    });
                }
            }
            _ => return Err("action not implemented for playing phase".into()),
        }

        // If the next leader has no cards left, pass lead to partner (per rules).
        if hand.trick.top_play.is_none() && hand.remaining_count(state.turn_seat) == 0 {
            let p = partner(state.turn_seat);
            if hand.remaining_count(p) > 0 {
                state.turn_seat = p;
                state.leader_seat = p;
            }
        }

        // Hand end: one team both empty; that team wins the hand.
        if is_team_empty(hand, TeamId::Ew) || is_team_empty(hand, TeamId::Sn) {
            state.phase = GamePhase::Scoring;
            state.winner_team = if is_team_empty(hand, TeamId::Ew) {
                Some(TeamId::Ew)
            } else {
                Some(TeamId::Sn)
            };
        }

        Ok(())
    }
}

fn ensure_tribute_phase_ready(hand: &HandState) -> Result<(), String> {
    if hand.tribute.is_some() {
        Ok(())
    } else {
        Err("missing tribute plan".into())
    }
}

fn ensure_is_highest_non_wild(hand: &HandState, seat: Seat, card: &str) -> Result<(), String> {
    let ctx = RuleContext {
        hand_level: hand.hand_level,
    };
    let parsed = parse_card_symbol(card)?;
    let h = hand
        .hands
        .get(&seat)
        .ok_or_else(|| "missing seat".to_string())?;
    if !h.iter().any(|c| c == card) {
        return Err("tribute card not in hand".into());
    }
    if is_wild(parsed, ctx) {
        return Err("tribute cannot be wild card".into());
    }
    let target = level_order_value(parsed, ctx);
    for s in h {
        let c = parse_card_symbol(s)?;
        if is_wild(c, ctx) {
            continue;
        }
        if level_order_value(c, ctx) > target {
            return Err("tribute must be highest non-wild single".into());
        }
    }
    Ok(())
}

fn seat_team(seat: Seat) -> TeamId {
    match seat {
        Seat::E | Seat::W => TeamId::Ew,
        Seat::S | Seat::N => TeamId::Sn,
    }
}

fn build_tribute_plan(
    hand: &HandState,
    declarer: TeamId,
    finishing: &[Seat],
) -> Result<TributeState, String> {
    let winner_first = *finishing
        .first()
        .ok_or_else(|| "empty finishing order".to_string())?;
    let winner_second = *finishing
        .get(1)
        .ok_or_else(|| "finishing order too short".to_string())?;
    let loser_third = *finishing
        .get(2)
        .ok_or_else(|| "finishing order too short".to_string())?;
    let loser_last = *finishing
        .get(3)
        .ok_or_else(|| "finishing order too short".to_string())?;

    let win_type = if seat_team(winner_second) == seat_team(winner_first) {
        WinCase::OneTwo
    } else if seat_team(loser_third) == seat_team(winner_first) {
        WinCase::OneThree
    } else {
        WinCase::OneFour
    };

    let mut pairs = Vec::new();
    match win_type {
        WinCase::OneTwo => {
            // losers (3rd,4th) each tribute to winners (1st,2nd)
            pairs.push(TributePair {
                payer: loser_third,
                receiver: winner_first,
                paid_card: None,
                return_card: None,
            });
            pairs.push(TributePair {
                payer: loser_last,
                receiver: winner_second,
                paid_card: None,
                return_card: None,
            });
        }
        WinCase::OneThree | WinCase::OneFour => {
            pairs.push(TributePair {
                payer: loser_last,
                receiver: winner_first,
                paid_card: None,
                return_card: None,
            });
        }
    }

    // cancel rule: all payers collectively have >=2 red jokers.
    let mut red = 0usize;
    for p in &pairs {
        if let Some(cards) = hand.hands.get(&p.payer) {
            for c in cards {
                if parse_card_symbol(c)?.rank == crate::game::card::Rank::RedJoker {
                    red += 1;
                }
            }
        }
    }
    let canceled = red >= 2;
    let opening_lead_candidates = if canceled {
        vec![winner_first]
    } else {
        pairs.iter().map(|p| p.payer).collect()
    };

    let _ = declarer; // reserved for future declarer-specific variants.
    Ok(TributeState {
        pairs,
        canceled,
        opening_lead_candidates,
    })
}

#[derive(Clone, Copy)]
enum WinCase {
    OneTwo,
    OneThree,
    OneFour,
}

fn tie_break_opening_leader(reference: Seat, candidates: &[Seat]) -> Seat {
    let order = [
        reference,
        next_ccw(reference),
        next_ccw(next_ccw(reference)),
        next_ccw(next_ccw(next_ccw(reference))),
    ];
    for s in order {
        if candidates.contains(&s) {
            return s;
        }
    }
    candidates[0]
}

fn next_ccw(seat: Seat) -> Seat {
    match seat {
        Seat::E => Seat::N,
        Seat::N => Seat::W,
        Seat::W => Seat::S,
        Seat::S => Seat::E,
    }
}

fn next_active_ccw(hand: &HandState, seat: Seat) -> Seat {
    let mut cur = next_ccw(seat);
    for _ in 0..Seat::ALL.len() {
        if hand.remaining_count(cur) > 0 {
            return cur;
        }
        cur = next_ccw(cur);
    }
    seat
}

fn apply_initial_phase_from_tribute_plan(
    state: &mut TableGameState,
    tribute: &TributeState,
) -> Result<(), String> {
    if tribute.canceled {
        let lead = tribute
            .opening_lead_candidates
            .first()
            .copied()
            .ok_or_else(|| "missing opening lead when tribute canceled".to_string())?;
        state.phase = GamePhase::Playing;
        state.turn_seat = lead;
        state.leader_seat = lead;
    } else {
        state.phase = GamePhase::Tribute;
    }
    Ok(())
}

fn passes_required_to_end_trick(hand: &HandState, last_play: Option<Seat>) -> u8 {
    let active = Seat::ALL
        .into_iter()
        .filter(|&s| hand.remaining_count(s) > 0)
        .count();
    if active == 0 {
        return 0;
    }
    let leader_has_cards = last_play.is_some_and(|s| hand.remaining_count(s) > 0);
    let required = if leader_has_cards {
        active.saturating_sub(1)
    } else {
        active
    };
    required as u8
}

fn partner(seat: Seat) -> Seat {
    match seat {
        Seat::E => Seat::W,
        Seat::W => Seat::E,
        Seat::S => Seat::N,
        Seat::N => Seat::S,
    }
}

fn is_team_empty(hand: &HandState, team: TeamId) -> bool {
    match team {
        TeamId::Ew => hand.remaining_count(Seat::E) == 0 && hand.remaining_count(Seat::W) == 0,
        TeamId::Sn => hand.remaining_count(Seat::S) == 0 && hand.remaining_count(Seat::N) == 0,
    }
}

fn remove_cards_from_hand(
    hands: &mut std::collections::HashMap<Seat, Vec<String>>,
    seat: Seat,
    cards: &[String],
) -> Result<(), String> {
    let h = hands
        .get_mut(&seat)
        .ok_or_else(|| "missing seat hand".to_string())?;
    // multiset removal: for each card symbol, remove one occurrence.
    for c in cards {
        if let Some(pos) = h.iter().position(|x| x == c) {
            h.remove(pos);
        } else {
            return Err(format!("card {:?} not in hand", c));
        }
    }
    Ok(())
}

fn ensure_cards_in_hand_multiset(
    hands: &std::collections::HashMap<Seat, Vec<String>>,
    seat: Seat,
    cards: &[String],
) -> Result<(), String> {
    let h = hands
        .get(&seat)
        .ok_or_else(|| "missing seat hand".to_string())?;
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for c in h {
        *counts.entry(c.as_str()).or_insert(0) += 1;
    }
    for c in cards {
        match counts.get_mut(c.as_str()) {
            Some(v) if *v > 0 => *v -= 1,
            _ => return Err(format!("card {:?} not in hand", c)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::card::HandLevel;
    use crate::game::rules::combination_parser::CombinationParser;
    use crate::game::test_support::TestFixtures;
    use crate::game::types::{HandState, PlayState};

    fn mk_state_playing() -> TableGameState {
        let mut s = TableGameState::new("t".into());
        s.phase = GamePhase::Playing;
        s.turn_seat = Seat::E;
        s.leader_seat = Seat::E;
        let mut hand = HandState::new(HandLevel::Three);
        // Give E a simple pair, others empty to simplify.
        hand.hands.insert(Seat::E, vec!["♠3".into(), "♦3".into()]);
        hand.hands.insert(Seat::S, vec![]);
        hand.hands.insert(Seat::W, vec![]);
        hand.hands.insert(Seat::N, vec![]);
        s.hand = Some(hand);
        s
    }

    fn set_top_play(hand: &mut HandState, seat: Seat, cards: Vec<&str>) {
        let cards: Vec<String> = cards.into_iter().map(String::from).collect();
        let combo = CombinationParser::parse(
            &cards,
            None,
            RuleContext {
                hand_level: hand.hand_level,
            },
        )
        .unwrap();
        hand.trick.top_play = Some(PlayState {
            seat,
            cards,
            wild_targets: None,
            combination: combo,
        });
        hand.trick.last_play_seat = Some(seat);
    }

    #[test]
    fn cannot_pass_when_leading_with_cards() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = mk_state_playing();
        let err = eng
            .apply_player_action(&mut s, Seat::E, PlayerAction::Pass, None)
            .unwrap_err();
        assert!(err.contains("cannot pass"));
    }

    #[test]
    fn tribute_requires_highest_non_wild() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = TableGameState::new("t".into());
        let mut hand = HandState::new(HandLevel::Two);
        hand.hands.insert(Seat::E, vec!["♠A".into(), "♣3".into()]);
        hand.hands.insert(Seat::S, vec!["♠4".into()]);
        hand.hands.insert(Seat::W, vec!["♠5".into()]);
        hand.hands.insert(Seat::N, vec!["♠6".into()]);
        hand.tribute = Some(TributeState {
            pairs: vec![TributePair {
                payer: Seat::E,
                receiver: Seat::S,
                paid_card: None,
                return_card: None,
            }],
            canceled: false,
            opening_lead_candidates: vec![Seat::E],
        });
        s.phase = GamePhase::Tribute;
        s.hand = Some(hand);
        let err = eng
            .apply_player_action(
                &mut s,
                Seat::E,
                PlayerAction::Tribute {
                    card: "♣3".into()
                },
                None,
            )
            .unwrap_err();
        assert!(err.contains("highest non-wild"));
    }

    #[test]
    fn canceled_tribute_starts_directly_in_playing() {
        let mut s = TableGameState::new("t_cancel".into());
        s.phase = GamePhase::Dealing;
        s.turn_seat = Seat::W;
        s.leader_seat = Seat::W;
        let tribute = TributeState {
            pairs: vec![TributePair {
                payer: Seat::N,
                receiver: Seat::E,
                paid_card: None,
                return_card: None,
            }],
            canceled: true,
            opening_lead_candidates: vec![Seat::E],
        };
        apply_initial_phase_from_tribute_plan(&mut s, &tribute).unwrap();
        assert_eq!(s.phase, GamePhase::Playing);
        assert_eq!(s.turn_seat, Seat::E);
        assert_eq!(s.leader_seat, Seat::E);
    }

    #[test]
    fn non_canceled_tribute_enters_tribute_phase() {
        let mut s = TableGameState::new("t_normal".into());
        s.phase = GamePhase::Dealing;
        s.turn_seat = Seat::S;
        s.leader_seat = Seat::S;
        let tribute = TributeState {
            pairs: vec![TributePair {
                payer: Seat::W,
                receiver: Seat::E,
                paid_card: None,
                return_card: None,
            }],
            canceled: false,
            opening_lead_candidates: vec![Seat::W],
        };
        apply_initial_phase_from_tribute_plan(&mut s, &tribute).unwrap();
        assert_eq!(s.phase, GamePhase::Tribute);
        assert_eq!(s.turn_seat, Seat::S);
        assert_eq!(s.leader_seat, Seat::S);
    }

    #[test]
    fn next_hand_uses_passed_hand_level() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = TableGameState::new("t_level".into());
        eng.start_next_hand_with_tribute(
            &mut s,
            TeamId::Ew,
            HandLevel::Eight,
            &[Seat::E, Seat::S, Seat::W, Seat::N],
        )
        .unwrap();
        assert_eq!(s.hand.as_ref().unwrap().hand_level, HandLevel::Eight);
    }

    #[test]
    fn invalid_parse_does_not_consume_cards() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = mk_state_playing();
        let hand = s.hand.as_mut().unwrap();
        hand.hands.insert(
            Seat::E,
            vec!["♠K".into(), "♥K".into(), "♦K".into(), "♠A".into()],
        );
        set_top_play(hand, Seat::N, vec!["♠7", "♥7", "♦7", "♣7"]);

        let before = s
            .hand
            .as_ref()
            .unwrap()
            .hands
            .get(&Seat::E)
            .unwrap()
            .clone();
        let err = eng
            .apply_player_action(
                &mut s,
                Seat::E,
                PlayerAction::Play {
                    cards: vec!["♠K".into(), "♥K".into(), "♦K".into(), "♠A".into()],
                    wild_targets: None,
                },
                None,
            )
            .unwrap_err();
        assert!(err.contains("unsupported combination length"));
        let after = s
            .hand
            .as_ref()
            .unwrap()
            .hands
            .get(&Seat::E)
            .unwrap()
            .clone();
        assert_eq!(after, before);
    }

    #[test]
    fn cannot_beat_does_not_consume_cards() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = mk_state_playing();
        let hand = s.hand.as_mut().unwrap();
        hand.hands.insert(Seat::E, vec!["♠6".into(), "♦3".into()]);
        set_top_play(hand, Seat::N, vec!["♠7"]);

        let before = s
            .hand
            .as_ref()
            .unwrap()
            .hands
            .get(&Seat::E)
            .unwrap()
            .clone();
        let err = eng
            .apply_player_action(
                &mut s,
                Seat::E,
                PlayerAction::Play {
                    cards: vec!["♠6".into()],
                    wild_targets: None,
                },
                None,
            )
            .unwrap_err();
        assert!(err.contains("does not beat"));
        let after = s
            .hand
            .as_ref()
            .unwrap()
            .hands
            .get(&Seat::E)
            .unwrap()
            .clone();
        assert_eq!(after, before);
    }

    #[test]
    fn duplicate_card_request_does_not_partially_consume_hand() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = mk_state_playing();

        let before = s
            .hand
            .as_ref()
            .unwrap()
            .hands
            .get(&Seat::E)
            .unwrap()
            .clone();
        let err = eng
            .apply_player_action(
                &mut s,
                Seat::E,
                PlayerAction::Play {
                    cards: vec!["♠3".into(), "♠3".into()],
                    wild_targets: None,
                },
                None,
            )
            .unwrap_err();
        assert!(err.contains("not in hand"));
        let after = s
            .hand
            .as_ref()
            .unwrap()
            .hands
            .get(&Seat::E)
            .unwrap()
            .clone();
        assert_eq!(after, before);
    }

    #[test]
    fn auto_infers_wild_targets_when_missing() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = mk_state_playing();
        let hand = s.hand.as_mut().unwrap();
        hand.hands.insert(Seat::E, vec!["♥3".into()]);

        eng.apply_player_action(
            &mut s,
            Seat::E,
            PlayerAction::Play {
                cards: vec!["♥3".into()],
                wild_targets: None,
            },
            None,
        )
        .unwrap();

        let top = &s.hand.as_ref().unwrap().trick.top_play.as_ref().unwrap();
        assert!(top.wild_targets.is_some());
        assert_eq!(top.wild_targets.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn auto_infer_respects_beat_for_follow_play() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = mk_state_playing();
        let hand = s.hand.as_mut().unwrap();
        hand.hands.insert(Seat::E, vec!["♥3".into(), "♠J".into()]);
        set_top_play(hand, Seat::N, vec!["♠10", "♦10"]);

        eng.apply_player_action(
            &mut s,
            Seat::E,
            PlayerAction::Play {
                cards: vec!["♥3".into(), "♠J".into()],
                wild_targets: None,
            },
            None,
        )
        .unwrap();

        let top = &s.hand.as_ref().unwrap().trick.top_play.as_ref().unwrap();
        assert!(top.wild_targets.is_some());
        assert_eq!(
            top.combination.kind,
            crate::game::rules::combination_parser::CombinationKind::Ordinary(
                crate::game::rules::combination_parser::OrdinaryKind::Pair
            )
        );
    }

    #[test]
    fn played_cards_are_sorted_for_public_state_and_history() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = TableGameState::new("t_sorted_play".into());
        s.phase = GamePhase::Playing;
        s.turn_seat = Seat::E;
        s.leader_seat = Seat::E;
        let mut hand = HandState::new(HandLevel::Two);
        hand.hands
            .insert(Seat::E, vec!["♣A".into(), "♥A".into(), "♠A".into()]);
        hand.hands.insert(Seat::S, vec![]);
        hand.hands.insert(Seat::W, vec![]);
        hand.hands.insert(Seat::N, vec![]);
        s.hand = Some(hand);

        eng.apply_player_action(
            &mut s,
            Seat::E,
            PlayerAction::Play {
                cards: vec!["♣A".into(), "♥A".into(), "♠A".into()],
                wild_targets: None,
            },
            Some(HandCommitMeta {
                seq: 1,
                timestamp: "2026-01-01T00:00:00Z".into(),
            }),
        )
        .unwrap();

        let hand = s.hand.as_ref().unwrap();
        let top = hand.trick.top_play.as_ref().unwrap();
        assert_eq!(top.cards, vec!["♥A", "♠A", "♣A"]);
        assert_eq!(hand.history.len(), 1);
        assert_eq!(hand.history[0].cards, vec!["♥A", "♠A", "♣A"]);
    }

    #[test]
    fn next_turn_skips_empty_seat_after_player_finishes() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = TableGameState::new("t_skip".into());
        s.phase = GamePhase::Playing;
        s.turn_seat = Seat::E;
        s.leader_seat = Seat::E;
        let mut hand = HandState::new(HandLevel::Two);
        hand.hands.insert(Seat::E, vec!["♠3".into()]);
        hand.hands.insert(Seat::N, vec![]);
        hand.hands.insert(Seat::W, vec!["♠4".into()]);
        hand.hands.insert(Seat::S, vec!["♠5".into()]);
        s.hand = Some(hand);

        eng.apply_player_action(
            &mut s,
            Seat::E,
            PlayerAction::Play {
                cards: vec!["♠3".into()],
                wild_targets: None,
            },
            None,
        )
        .unwrap();

        assert_eq!(s.turn_seat, Seat::W);
        let hand = s.hand.as_ref().unwrap();
        assert_eq!(hand.finishing_order, vec![Seat::E]);
    }

    #[test]
    fn trick_ends_after_all_remaining_active_players_pass() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = TableGameState::new("t_pass".into());
        s.phase = GamePhase::Playing;
        s.turn_seat = Seat::N;
        s.leader_seat = Seat::E;
        let mut hand = HandState::new(HandLevel::Two);
        hand.hands.insert(Seat::E, vec![]);
        hand.hands.insert(Seat::N, vec!["♠7".into()]);
        hand.hands.insert(Seat::W, vec!["♠8".into()]);
        hand.hands.insert(Seat::S, vec![]);
        set_top_play(&mut hand, Seat::E, vec!["♠6"]);
        s.hand = Some(hand);

        eng.apply_player_action(&mut s, Seat::N, PlayerAction::Pass, None)
            .unwrap();
        assert_eq!(s.turn_seat, Seat::W);
        assert!(s.hand.as_ref().unwrap().trick.top_play.is_some());

        eng.apply_player_action(&mut s, Seat::W, PlayerAction::Pass, None)
            .unwrap();
        let hand = s.hand.as_ref().unwrap();
        assert!(hand.trick.top_play.is_none());
        assert_eq!(hand.trick.consecutive_passes, 0);
        // Last play seat E is empty, so lead is transferred to partner W.
        assert_eq!(s.turn_seat, Seat::W);
    }

    #[test]
    fn four_singles_endgame_script_reaches_scoring() {
        let eng = GameEngine::new(GameConfig::default());
        let mut s = TestFixtures::table_game_playing_four_singles_endgame();
        assert_eq!(s.phase, GamePhase::Playing);
        eng.apply_player_action(
            &mut s,
            Seat::E,
            PlayerAction::Play {
                cards: vec!["♠3".into()],
                wild_targets: None,
            },
            None,
        )
        .unwrap();
        eng.apply_player_action(
            &mut s,
            Seat::N,
            PlayerAction::Play {
                cards: vec!["♠6".into()],
                wild_targets: None,
            },
            None,
        )
        .unwrap();
        eng.apply_player_action(&mut s, Seat::W, PlayerAction::Pass, None)
            .unwrap();
        eng.apply_player_action(&mut s, Seat::S, PlayerAction::Pass, None)
            .unwrap();
        eng.apply_player_action(
            &mut s,
            Seat::S,
            PlayerAction::Play {
                cards: vec!["♠5".into()],
                wild_targets: None,
            },
            None,
        )
        .unwrap();
        // S's last card empties both S and N -> SN team wins -> scoring (no further passes).
        assert_eq!(s.phase, GamePhase::Scoring);
        assert_eq!(s.winner_team, Some(TeamId::Sn));
    }
}
