//! Domain types aligned with [doc/design.md](../doc/design.md) — MVP subset.

use crate::game::card::{sort_card_symbols_desc, HandLevel};
use crate::game::rules::scoring::{Level, TeamProgress};
use crate::game::types::{GameConfig, GamePhase, HistoryActionKind, TableGameState, TeamId};
use serde::de::Error as SerdeDeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Seat position in turn order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Seat {
    E,
    S,
    W,
    N,
}

impl Seat {
    pub const ALL: [Seat; 4] = [Seat::E, Seat::S, Seat::W, Seat::N];

    pub fn as_str(self) -> &'static str {
        match self {
            Seat::E => "E",
            Seat::S => "S",
            Seat::W => "W",
            Seat::N => "N",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableStatus {
    Waiting,
    InGame,
    Finished,
}

impl Serialize for TableStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            TableStatus::Waiting => "waiting",
            TableStatus::InGame => "in_game",
            TableStatus::Finished => "finished",
        })
    }
}

impl<'de> Deserialize<'de> for TableStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "waiting" => Ok(TableStatus::Waiting),
            "in_game" => Ok(TableStatus::InGame),
            "finished" => Ok(TableStatus::Finished),
            _ => Err(SerdeDeError::custom("invalid table status")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    TableSetup,
    Dealing,
    Tribute,
    Exchange,
    Playing,
    Scoring,
    Completed,
}

impl Serialize for Phase {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Phase::TableSetup => "table_setup",
            Phase::Dealing => "dealing",
            Phase::Tribute => "tribute",
            Phase::Exchange => "exchange",
            Phase::Playing => "playing",
            Phase::Scoring => "scoring",
            Phase::Completed => "completed",
        })
    }
}

impl<'de> Deserialize<'de> for Phase {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "table_setup" => Ok(Phase::TableSetup),
            "dealing" => Ok(Phase::Dealing),
            "tribute" => Ok(Phase::Tribute),
            "exchange" => Ok(Phase::Exchange),
            "playing" => Ok(Phase::Playing),
            "scoring" => Ok(Phase::Scoring),
            "completed" => Ok(Phase::Completed),
            _ => Err(SerdeDeError::custom("invalid phase")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum PlayerType {
    Human,
    Ai,
    #[default]
    Unknown,
}

impl Serialize for PlayerType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            PlayerType::Human => "human",
            PlayerType::Ai => "ai",
            PlayerType::Unknown => "unknown",
        })
    }
}

impl<'de> Deserialize<'de> for PlayerType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "human" => Ok(PlayerType::Human),
            "ai" => Ok(PlayerType::Ai),
            "unknown" => Ok(PlayerType::Unknown),
            _ => Err(SerdeDeError::custom("invalid player type")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatPublic {
    pub player_id: Option<String>,
    pub player_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_type: Option<PlayerType>,
    pub ready: bool,
    /// Remaining hand cards; null in MVP lobby.
    pub remaining_count: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPublic {
    pub team_id: String,
    pub seats: Vec<String>,
    pub level: String,
    pub ace_failed_attempts: u32,
    pub role: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expect {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_player_id: Option<String>,
    #[serde(default)]
    pub legal_actions: Vec<String>,
    pub deadline_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scoreboard {
    #[serde(default)]
    pub finishing_order: Vec<String>,
    pub last_hand_result: Option<serde_json::Value>,
    pub game_winner_team_id: Option<String>,
}

/// Full public table state at a given `seq`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableState {
    pub table_id: String,
    pub seq: u64,
    pub status: TableStatus,
    pub phase: Phase,
    pub narration: String,
    pub seats: HashMap<String, SeatPublic>,
    pub teams: Vec<TeamPublic>,
    pub hand: Option<serde_json::Value>,
    pub expect: Expect,
    pub scoreboard: Scoreboard,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateView {
    pub player_id: String,
    pub seat: String,
    #[serde(default)]
    pub hand_cards: Vec<String>,
    pub play_hints: PlayHints,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayHints {
    pub can_play: bool,
    pub can_pass: bool,
}

/// One committed transition envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateTransition {
    pub seq: u64,
    pub prev_seq: u64,
    pub table_id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub transition_type: String,
    pub delta: TransitionDelta,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionDelta {
    pub ops: Vec<serde_json::Value>,
    #[serde(default)]
    pub private_ops: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
}

/// Response body for `nextstate` (includes expect + prompt).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NextStateBody {
    #[serde(flatten)]
    pub transition: StateTransition,
    /// `server_current_seq - transition.seq` at response build time (0 = caught up to head).
    pub lag: u64,
    pub expect: Expect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private: Option<PrivateView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

// --- Runtime table (not directly serialized as API) ---

#[derive(Clone, Debug)]
pub struct PlayerRecord {
    pub player_id: String,
    pub player_name: String,
    pub player_type: PlayerType,
    pub ready: bool,
}

#[derive(Clone, Debug)]
pub struct TableRuntimeState {
    pub table_id: String,
    pub table_name: Option<String>,
    pub seats: HashMap<Seat, Option<PlayerRecord>>,
    pub status: TableStatus,
    pub phase: Phase,
    pub narration: String,
    pub seq: u64,
    /// RNG seed for dealing / shuffling; set when the first hand starts.
    pub game_config: GameConfig,
    /// Team progress persisted across hands.
    pub team_progress_ew: TeamProgress,
    pub team_progress_sn: TeamProgress,
    /// Declarer team for the current/next hand.
    pub current_declarer: TeamId,
    /// When true, hand has ended and table waits for all players ready.
    pub waiting_next_hand_ready: bool,
    /// Last hand finishing order (completed to 4 seats when scoring is applied).
    pub last_finishing_order: Vec<Seat>,
    /// Authoritative game state when `status == InGame` (or after); `None` in lobby.
    pub game: Option<TableGameState>,
}

impl TableRuntimeState {
    pub fn new(table_id: String, table_name: Option<String>) -> Self {
        let mut seats = HashMap::new();
        for s in Seat::ALL {
            seats.insert(s, None);
        }
        Self {
            table_id,
            table_name,
            seats,
            status: TableStatus::Waiting,
            phase: Phase::TableSetup,
            narration: String::new(),
            seq: 0,
            game_config: GameConfig::default(),
            team_progress_ew: TeamProgress {
                team: TeamId::Ew,
                level: Level::Two,
                ace_failed_attempts: 0,
            },
            team_progress_sn: TeamProgress {
                team: TeamId::Sn,
                level: Level::Two,
                ace_failed_attempts: 0,
            },
            current_declarer: TeamId::Ew,
            waiting_next_hand_ready: false,
            last_finishing_order: Vec::new(),
            game: None,
        }
    }

    /// Stable seed derived from `table_id` for deterministic tests and reproducible deals.
    pub fn hash_table_id_seed(table_id: &str) -> u64 {
        let mut h = DefaultHasher::new();
        table_id.hash(&mut h);
        h.finish()
    }

    pub fn game_phase_to_domain(gp: GamePhase) -> Phase {
        match gp {
            GamePhase::TableSetup => Phase::TableSetup,
            GamePhase::Dealing => Phase::Dealing,
            GamePhase::Tribute => Phase::Tribute,
            GamePhase::Exchange => Phase::Exchange,
            GamePhase::Playing => Phase::Playing,
            GamePhase::Scoring => Phase::Scoring,
            GamePhase::Completed => Phase::Completed,
        }
    }

    pub fn sync_phase_from_game(&mut self) {
        if let Some(g) = &self.game {
            self.phase = Self::game_phase_to_domain(g.phase);
        }
    }

    pub fn player_id_for_seat(&self, seat: Seat) -> Option<String> {
        self.seats
            .get(&seat)
            .and_then(|o| o.as_ref())
            .map(|p| p.player_id.clone())
    }

    pub fn seat_for_player(&self, player_id: &str) -> Option<Seat> {
        Seat::ALL.into_iter().find(|s| {
            self.seats
                .get(s)
                .and_then(|o| o.as_ref())
                .is_some_and(|p| p.player_id == player_id)
        })
    }

    fn hand_level_to_api(hl: HandLevel) -> &'static str {
        match hl {
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

    fn level_to_api(level: Level) -> &'static str {
        match level {
            Level::Two => "2",
            Level::Three => "3",
            Level::Four => "4",
            Level::Five => "5",
            Level::Six => "6",
            Level::Seven => "7",
            Level::Eight => "8",
            Level::Nine => "9",
            Level::Ten => "10",
            Level::J => "J",
            Level::Q => "Q",
            Level::K => "K",
            Level::A => "A",
        }
    }

    fn materialize_hand_json(&self, g: &TableGameState) -> serde_json::Value {
        let Some(hand) = g.hand.as_ref() else {
            return json!(null);
        };
        let stage = match g.phase {
            GamePhase::Dealing => "dealing",
            GamePhase::Tribute => "tribute",
            GamePhase::Exchange => "exchange",
            GamePhase::Playing => "playing",
            GamePhase::Scoring => "scoring",
            GamePhase::Completed => "completed",
            GamePhase::TableSetup => "table_setup",
        };
        let tribute_plan = hand.tribute.as_ref().map(|t| {
            json!({
                "canceled": t.canceled,
                "pairs": t.pairs.iter().map(|p| json!({
                    "payer": p.payer.as_str(),
                    "receiver": p.receiver.as_str(),
                    "paidCard": p.paid_card,
                    "returnCard": p.return_card,
                })).collect::<Vec<_>>()
            })
        });
        let top_play = hand.trick.top_play.as_ref().map(|p| {
            json!({
                "seat": p.seat.as_str(),
                "cards": p.cards,
                "wildTargets": p.wild_targets,
                "combinationKind": format!("{:?}", p.combination.kind),
            })
        });
        let history: Vec<serde_json::Value> = hand
            .history
            .iter()
            .map(|e| {
                let action_type = match e.action_type {
                    HistoryActionKind::Play => "play",
                    HistoryActionKind::Pass => "pass",
                };
                let declared = match &e.wild_targets {
                    Some(wt) if !wt.is_empty() => json!({ "wildTargets": wt }),
                    _ => json!({}),
                };
                match e.action_type {
                    HistoryActionKind::Play => json!({
                        "seq": e.seq,
                        "actionId": e.action_id,
                        "seat": e.seat.as_str(),
                        "actionType": action_type,
                        "combinationType": e.combination_type,
                        "cards": e.cards,
                        "declaredWildMapping": declared,
                        "timestamp": e.timestamp,
                    }),
                    HistoryActionKind::Pass => json!({
                        "seq": e.seq,
                        "actionId": e.action_id,
                        "seat": e.seat.as_str(),
                        "actionType": action_type,
                        "timestamp": e.timestamp,
                    }),
                }
            })
            .collect();
        json!({
            "handId": format!("h_{}_{}", g.table_id, g.hand_index),
            "handIndex": g.hand_index,
            "handLevel": Self::hand_level_to_api(hand.hand_level),
            "dealerSeat": g.dealer_seat.as_str(),
            "leaderSeat": g.leader_seat.as_str(),
            "turnSeat": g.turn_seat.as_str(),
            "trickIndex": 0,
            "stage": stage,
            "tributePlan": tribute_plan,
            "finishingOrder": hand.finishing_order.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            "winnerTeamId": g.winner_team.map(|t| t.as_str()),
            "history": history,
            "topPlay": top_play,
            "startedAt": null,
            "endedAt": null
        })
    }

    fn compute_expect_in_game(&self, g: &TableGameState) -> Expect {
        match g.phase {
            GamePhase::Tribute => {
                let actor = g.hand.as_ref().and_then(|h| {
                    if h.tribute.as_ref().is_some_and(|t| t.canceled) {
                        h.tribute
                            .as_ref()
                            .and_then(|t| t.opening_lead_candidates.first().copied())
                    } else {
                        h.next_tribute_actor()
                    }
                });
                Expect {
                    kind: "tribute".into(),
                    actor_player_id: actor.and_then(|s| self.player_id_for_seat(s)),
                    legal_actions: vec!["tribute".into()],
                    deadline_at: None,
                }
            }
            GamePhase::Exchange => {
                let actor = g.hand.as_ref().and_then(|h| h.next_exchange_actor());
                Expect {
                    kind: "exchange".into(),
                    actor_player_id: actor.and_then(|s| self.player_id_for_seat(s)),
                    legal_actions: vec!["return_card".into()],
                    deadline_at: None,
                }
            }
            GamePhase::Playing => Expect {
                kind: "play".into(),
                actor_player_id: self.player_id_for_seat(g.turn_seat),
                legal_actions: vec!["play".into(), "pass".into()],
                deadline_at: None,
            },
            GamePhase::Scoring | GamePhase::Dealing | GamePhase::TableSetup => Expect {
                kind: "wait".into(),
                actor_player_id: None,
                legal_actions: vec![],
                deadline_at: None,
            },
            GamePhase::Completed => Expect {
                kind: "game_over".into(),
                actor_player_id: None,
                legal_actions: vec![],
                deadline_at: None,
            },
        }
    }

    /// Private cards and hints for `player_id` when seated (used by snapshot / nextstate).
    pub fn private_view_for_player(&self, player_id: &str) -> Option<PrivateView> {
        let seat = self.seat_for_player(player_id)?;
        let hand_level = self
            .game
            .as_ref()
            .and_then(|g| g.hand.as_ref())
            .map(|h| h.hand_level)
            .unwrap_or(HandLevel::Two);
        let mut hand_cards = self
            .game
            .as_ref()
            .and_then(|g| g.hand.as_ref())
            .and_then(|h| h.hands.get(&seat).cloned())
            .unwrap_or_default();
        sort_private_hand_cards_desc(&mut hand_cards, hand_level);
        let expect = self.compute_expect();
        let mine = expect.actor_player_id.as_deref() == Some(player_id);
        let (can_play, can_pass) = if expect.kind == "play" {
            (mine, mine)
        } else {
            (false, false)
        };
        Some(PrivateView {
            player_id: player_id.to_string(),
            seat: seat.as_str().to_string(),
            hand_cards,
            play_hints: PlayHints {
                can_play,
                can_pass,
            },
        })
    }

    pub fn occupied_count(&self) -> usize {
        self.seats.values().filter(|o| o.is_some()).count()
    }

    pub fn all_ready(&self) -> bool {
        self.occupied_count() == 4
            && self
                .seats
                .values()
                .all(|o| o.as_ref().map(|p| p.ready).unwrap_or(false))
    }

    pub fn team_ew_id() -> String {
        "team_ew".to_string()
    }

    pub fn team_sn_id() -> String {
        "team_sn".to_string()
    }

    /// Materialize API [`TableState`] at `self.seq`.
    pub fn to_table_state(&self) -> TableState {
        let mut seats_json = HashMap::new();
        for seat in Seat::ALL {
            let key = seat.as_str().to_string();
            let sp = self.seats.get(&seat).and_then(|o| o.as_ref());
            let remaining_count = self
                .game
                .as_ref()
                .and_then(|g| g.hand.as_ref())
                .map(|h| h.remaining_count(seat) as u32);
            seats_json.insert(
                key,
                match sp {
                    Some(p) => SeatPublic {
                        player_id: Some(p.player_id.clone()),
                        player_name: Some(p.player_name.clone()),
                        player_type: Some(p.player_type.clone()),
                        ready: p.ready,
                        remaining_count,
                    },
                    None => SeatPublic {
                        player_id: None,
                        player_name: None,
                        player_type: None,
                        ready: false,
                        remaining_count: None,
                    },
                },
            );
        }

        let teams = vec![
            TeamPublic {
                team_id: Self::team_ew_id(),
                seats: vec!["E".into(), "W".into()],
                level: Self::level_to_api(self.team_progress_ew.level).into(),
                ace_failed_attempts: self.team_progress_ew.ace_failed_attempts,
                role: if self.current_declarer == TeamId::Ew {
                    "declarer".into()
                } else {
                    "opponent".into()
                },
            },
            TeamPublic {
                team_id: Self::team_sn_id(),
                seats: vec!["S".into(), "N".into()],
                level: Self::level_to_api(self.team_progress_sn.level).into(),
                ace_failed_attempts: self.team_progress_sn.ace_failed_attempts,
                role: if self.current_declarer == TeamId::Sn {
                    "declarer".into()
                } else {
                    "opponent".into()
                },
            },
        ];

        let expect = self.compute_expect();

        let hand = match (&self.status, &self.game) {
            (TableStatus::InGame, Some(g)) => Some(self.materialize_hand_json(g)),
            _ => None,
        };

        TableState {
            table_id: self.table_id.clone(),
            seq: self.seq,
            status: self.status.clone(),
            phase: self.phase.clone(),
            narration: self.narration.clone(),
            seats: seats_json,
            teams,
            hand,
            expect,
            scoreboard: Scoreboard::default(),
        }
    }

    pub(crate) fn compute_expect(&self) -> Expect {
        let occupied = self.occupied_count();
        if matches!(self.status, TableStatus::Finished) {
            return Expect {
                kind: "game_over".into(),
                actor_player_id: None,
                legal_actions: vec![],
                deadline_at: None,
            };
        }
        if occupied < 4 {
            return Expect {
                kind: "join".into(),
                actor_player_id: None,
                legal_actions: vec![],
                deadline_at: None,
            };
        }
        if !self.all_ready() {
            return Expect {
                kind: "ready".into(),
                actor_player_id: None,
                legal_actions: vec!["ready".into()],
                deadline_at: None,
            };
        }
        if matches!(self.status, TableStatus::InGame) {
            if let Some(ref g) = self.game {
                if matches!(g.phase, GamePhase::Scoring) && self.waiting_next_hand_ready {
                    return Expect {
                        kind: "ready".into(),
                        actor_player_id: None,
                        legal_actions: vec!["ready".into()],
                        deadline_at: None,
                    };
                }
                return self.compute_expect_in_game(g);
            }
            return Expect {
                kind: "wait".into(),
                actor_player_id: None,
                legal_actions: vec![],
                deadline_at: None,
            };
        }
        Expect {
            kind: "wait".into(),
            actor_player_id: None,
            legal_actions: vec![],
            deadline_at: None,
        }
    }
}

fn sort_private_hand_cards_desc(hand_cards: &mut [String], hand_level: HandLevel) {
    sort_card_symbols_desc(hand_cards, hand_level);
}

/// Encode one JSON Pointer path segment (RFC 6901).
fn json_pointer_encode_segment(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

/// When `history` only grows by a tail suffix and key set matches, emit `add` for new items
/// and `replace` for other changed `/hand/*` keys; otherwise `None` (caller uses full `/hand` replace).
fn try_hand_incremental_ops(prev_hand: &serde_json::Value, next_hand: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    let prev_obj = prev_hand.as_object()?;
    let next_obj = next_hand.as_object()?;
    let ph = prev_obj.get("history")?.as_array()?;
    let nh = next_obj.get("history")?.as_array()?;
    if nh.len() < ph.len() {
        return None;
    }
    for i in 0..ph.len() {
        if ph[i] != nh[i] {
            return None;
        }
    }
    for key in prev_obj.keys() {
        if key == "history" {
            continue;
        }
        if !next_obj.contains_key(key) {
            return None;
        }
    }

    let mut ops = Vec::new();
    for item in nh.iter().skip(ph.len()) {
        ops.push(json!({
            "op": "add",
            "path": "/hand/history/-",
            "value": item.clone(),
        }));
    }
    for (key, nval) in next_obj {
        if key == "history" {
            continue;
        }
        match prev_obj.get(key) {
            Some(pval) if pval == nval => {}
            _ => {
                let path = format!("/hand/{}", json_pointer_encode_segment(key));
                ops.push(json!({"op": "replace", "path": path, "value": nval.clone()}));
            }
        }
    }
    Some(ops)
}

/// Build a minimal delta: replace whole snapshot fields that change often; `hand` may use incremental ops.
pub fn snapshot_replace_delta(prev: &TableState, next: &TableState) -> TransitionDelta {
    let mut ops = vec![
        json!({"op": "replace", "path": "/tableId", "value": next.table_id}),
        json!({"op": "replace", "path": "/seq", "value": next.seq}),
        json!({"op": "replace", "path": "/status", "value": next.status}),
        json!({"op": "replace", "path": "/phase", "value": next.phase}),
        json!({"op": "replace", "path": "/narration", "value": next.narration}),
        json!({"op": "replace", "path": "/seats", "value": next.seats}),
        json!({"op": "replace", "path": "/teams", "value": next.teams}),
        json!({"op": "replace", "path": "/expect", "value": next.expect}),
        json!({"op": "replace", "path": "/scoreboard", "value": next.scoreboard}),
    ];
    match (prev.hand.as_ref(), next.hand.as_ref()) {
        (None, None) => {}
        (Some(_), None) | (None, Some(_)) => {
            ops.push(json!({"op": "replace", "path": "/hand", "value": next.hand}));
        }
        (Some(ph), Some(nh)) => {
            if let Some(hand_ops) = try_hand_incremental_ops(ph, nh) {
                ops.extend(hand_ops);
            } else {
                ops.push(json!({"op": "replace", "path": "/hand", "value": next.hand}));
            }
        }
    }
    TransitionDelta {
        ops,
        private_ops: vec![],
        event: None,
    }
}

fn json_pointer_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

fn apply_one_replace(root: &mut serde_json::Value, path: &str, value: serde_json::Value) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err(format!("path must start with /: {path:?}"));
    }
    let segments: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| json_pointer_token(s))
        .collect();

    if segments.is_empty() {
        *root = value;
        return Ok(());
    }

    let mut cur = root;
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i + 1 == segments.len();
        if is_last {
            let m = cur
                .as_object_mut()
                .ok_or_else(|| format!("replace target not an object at path {path:?}"))?;
            m.insert(seg.clone(), value);
            return Ok(());
        }
        let m = cur
            .as_object_mut()
            .ok_or_else(|| format!("cannot descend {seg:?} in {path:?}"))?;
        cur = m.entry(seg.clone()).or_insert_with(|| json!({}));
    }
    Ok(())
}

fn apply_one_add(root: &mut serde_json::Value, path: &str, value: serde_json::Value) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err(format!("path must start with /: {path:?}"));
    }
    let segments: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| json_pointer_token(s))
        .collect();
    if segments.last().map(|s| s.as_str()) != Some("-") {
        return Err(format!("add path must end with /-: {path:?}"));
    }
    if segments.len() < 2 {
        return Err(format!("add path too short: {path:?}"));
    }
    let parent_segs = &segments[..segments.len() - 1];
    let mut cur = root;
    for (i, seg) in parent_segs.iter().enumerate() {
        let is_last = i + 1 == parent_segs.len();
        let m = cur
            .as_object_mut()
            .ok_or_else(|| format!("add: not an object at parent segment {i} of {path:?}"))?;
        cur = if is_last {
            m.get_mut(seg).ok_or_else(|| format!("add: missing key {seg:?} in {path:?}"))?
        } else {
            m.entry(seg.clone()).or_insert_with(|| json!({}))
        };
    }
    let arr = cur
        .as_array_mut()
        .ok_or_else(|| format!("add: parent is not an array at {path:?}"))?;
    arr.push(value);
    Ok(())
}

/// Apply JSON Patch `replace` and `add` (`add` only for paths ending in `/-`, array append).
fn apply_delta_ops(root: &mut serde_json::Value, ops: &[serde_json::Value]) -> Result<(), String> {
    for op in ops {
        let kind = op
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("missing op: {op}"))?;
        let path = op
            .get("path")
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("missing path: {op}"))?;
        let value = op.get("value").cloned().ok_or_else(|| format!("missing value: {op}"))?;
        match kind {
            "replace" => apply_one_replace(root, path, value)?,
            "add" => apply_one_add(root, path, value)?,
            other => return Err(format!("unsupported op: {other}")),
        }
    }
    Ok(())
}

/// Apply server [`TransitionDelta`] `ops` onto a materialized [`TableState`] (`replace` and `add` for `/hand/history/-`).
pub fn apply_transition_delta_to_table_state(
    table: &TableState,
    delta: &TransitionDelta,
) -> Result<TableState, String> {
    let mut v = serde_json::to_value(table).map_err(|e| e.to_string())?;
    apply_delta_ops(&mut v, &delta.ops)?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

#[cfg(test)]
mod apply_delta_tests {
    use super::*;

    #[test]
    fn apply_replace_roundtrips_snapshot_delta() {
        let prev = TableRuntimeState::new("t_apply".into(), None).to_table_state();
        let mut next = prev.clone();
        next.seq = prev.seq.saturating_add(1);
        let delta = snapshot_replace_delta(&prev, &next);
        let got = apply_transition_delta_to_table_state(&prev, &delta).expect("apply");
        assert_eq!(
            serde_json::to_value(&got).unwrap(),
            serde_json::to_value(&next).unwrap()
        );
    }

    #[test]
    fn hand_history_tail_uses_add_and_field_replace() {
        let mut prev = TableRuntimeState::new("t_h".into(), None).to_table_state();
        prev.hand = Some(json!({
            "handId": "h_t_1",
            "history": [
                {"seq": 1, "actionId": "a_1", "seat": "E", "actionType": "play", "combinationType": "single", "cards": ["♠3"], "declaredWildMapping": {}, "timestamp": "t1"}
            ],
            "turnSeat": "N",
            "topPlay": null,
        }));
        let mut next = prev.clone();
        next.seq = prev.seq.saturating_add(1);
        next.hand = Some(json!({
            "handId": "h_t_1",
            "history": [
                {"seq": 1, "actionId": "a_1", "seat": "E", "actionType": "play", "combinationType": "single", "cards": ["♠3"], "declaredWildMapping": {}, "timestamp": "t1"},
                {"seq": 2, "actionId": "a_2", "seat": "N", "actionType": "pass", "timestamp": "t2"},
            ],
            "turnSeat": "W",
            "topPlay": null,
        }));
        let delta = snapshot_replace_delta(&prev, &next);
        assert!(
            delta.ops.iter().any(|o| {
                o.get("op").and_then(|x| x.as_str()) == Some("add")
                    && o.get("path").and_then(|x| x.as_str()) == Some("/hand/history/-")
            }),
            "expected add /hand/history/- in {:?}",
            delta.ops
        );
        assert!(
            delta.ops.iter().any(|o| {
                o.get("op").and_then(|x| x.as_str()) == Some("replace")
                    && o.get("path").and_then(|x| x.as_str()) == Some("/hand/turnSeat")
            }),
            "expected replace /hand/turnSeat in {:?}",
            delta.ops
        );
        let got = apply_transition_delta_to_table_state(&prev, &delta).expect("apply");
        assert_eq!(
            serde_json::to_value(&got).unwrap(),
            serde_json::to_value(&next).unwrap()
        );
    }

    #[test]
    fn hand_history_reset_falls_back_to_full_hand_replace() {
        let mut prev = TableRuntimeState::new("t_r".into(), None).to_table_state();
        prev.hand = Some(json!({
            "handId": "h_old",
            "history": [{"seq": 1, "actionId": "a_1", "seat": "E", "actionType": "pass", "timestamp": "t"}],
            "turnSeat": "E",
        }));
        let mut next = prev.clone();
        next.seq = prev.seq.saturating_add(1);
        next.hand = Some(json!({
            "handId": "h_new",
            "history": [],
            "turnSeat": "S",
        }));
        let delta = snapshot_replace_delta(&prev, &next);
        assert!(
            delta.ops.iter().any(|o| {
                o.get("op").and_then(|x| x.as_str()) == Some("replace")
                    && o.get("path").and_then(|x| x.as_str()) == Some("/hand")
            }),
            "expected replace /hand, got {:?}",
            delta.ops
        );
        let got = apply_transition_delta_to_table_state(&prev, &delta).expect("apply");
        assert_eq!(
            serde_json::to_value(&got).unwrap(),
            serde_json::to_value(&next).unwrap()
        );
    }

    #[test]
    fn private_hand_sort_respects_hand_level_then_suit() {
        let mut cards = vec![
            "♠A".to_string(),
            "♠5".to_string(),
            "♥5".to_string(),
            "🃏b".to_string(),
        ];
        sort_private_hand_cards_desc(&mut cards, HandLevel::Five);
        assert_eq!(cards, vec!["🃏b", "♥5", "♠5", "♠A"]);
    }
}

pub fn iso_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
