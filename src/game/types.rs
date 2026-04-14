use crate::domain::Seat;
use crate::game::card::HandLevel;
use crate::game::rules::combination_parser::Combination;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePhase {
    TableSetup,
    Dealing,
    Tribute,
    Exchange,
    Playing,
    Scoring,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamId {
    Ew,
    Sn,
}

impl TeamId {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamId::Ew => "team_ew",
            TeamId::Sn => "team_sn",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameConfig {
    pub rng_seed: u64,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self { rng_seed: 0 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayState {
    pub seat: Seat,
    pub cards: Vec<String>,
    pub wild_targets: Option<Vec<String>>,
    pub combination: Combination,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TrickState {
    pub top_play: Option<PlayState>,
    pub consecutive_passes: u8,
    pub last_play_seat: Option<Seat>,
}

/// Committed transition metadata for appending a [`HandHistoryEntry`] (Playing phase only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandCommitMeta {
    pub seq: u64,
    pub timestamp: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryActionKind {
    Play,
    Pass,
}

/// One public action in the current hand (API `hand.history` item).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandHistoryEntry {
    pub seq: u64,
    pub action_id: String,
    pub seat: Seat,
    pub timestamp: String,
    pub action_type: HistoryActionKind,
    pub cards: Vec<String>,
    pub combination_type: Option<String>,
    pub wild_targets: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandState {
    pub hand_level: HandLevel,
    pub hands: std::collections::HashMap<Seat, Vec<String>>,
    pub trick: TrickState,
    pub finishing_order: Vec<Seat>,
    pub tribute: Option<TributeState>,
    pub history: Vec<HandHistoryEntry>,
}

impl HandState {
    pub fn next_tribute_actor(&self) -> Option<Seat> {
        let t = self.tribute.as_ref()?;
        if t.canceled {
            return None;
        }
        t.pairs
            .iter()
            .find(|p| p.paid_card.is_none())
            .map(|p| p.payer)
    }

    pub fn next_exchange_actor(&self) -> Option<Seat> {
        let t = self.tribute.as_ref()?;
        if t.canceled {
            return None;
        }
        t.pairs
            .iter()
            .find(|p| p.return_card.is_none())
            .map(|p| p.receiver)
    }

    pub fn new(hand_level: HandLevel) -> Self {
        let mut hands = std::collections::HashMap::new();
        for s in Seat::ALL {
            hands.insert(s, Vec::new());
        }
        Self {
            hand_level,
            hands,
            trick: TrickState::default(),
            finishing_order: Vec::new(),
            tribute: None,
            history: Vec::new(),
        }
    }

    pub fn remaining_count(&self, seat: Seat) -> usize {
        self.hands.get(&seat).map(|v| v.len()).unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TributePair {
    pub payer: Seat,
    pub receiver: Seat,
    pub paid_card: Option<String>,
    pub return_card: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TributeState {
    pub pairs: Vec<TributePair>,
    pub canceled: bool,
    pub opening_lead_candidates: Vec<Seat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableGameState {
    pub table_id: String,
    pub phase: GamePhase,
    pub dealer_seat: Seat,
    pub leader_seat: Seat,
    pub turn_seat: Seat,
    pub hand_index: u32,
    pub hand: Option<HandState>,
    pub winner_team: Option<TeamId>,
}

impl TableGameState {
    pub fn new(table_id: String) -> Self {
        Self {
            table_id,
            phase: GamePhase::TableSetup,
            dealer_seat: Seat::E,
            leader_seat: Seat::S,
            turn_seat: Seat::E,
            hand_index: 0,
            hand: None,
            winner_team: None,
        }
    }
}

