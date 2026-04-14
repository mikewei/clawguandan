//! In-memory tables, per-table transition log, and long-poll signalling.

use crate::domain::{
    iso_timestamp, snapshot_replace_delta, Expect, NextStateBody, PlayerRecord, PlayerType,
    Seat, StateTransition, TableRuntimeState, TableState, TableStatus,
};
use crate::error::AppError;
use crate::game::card::HandLevel;
use crate::game::engine::{GameEngine, PlayerAction};
use crate::game::rules::narration::{
    format_big_play, format_hand_end, format_rank_announce, format_tribute_action,
    format_tribute_canceled,
    is_big_play_combination,
};
use crate::game::rules::scoring::{Level, ScoringService, WinType};
use crate::game::types::{GameConfig, GamePhase, HandCommitMeta, HandState, HistoryActionKind, TeamId};
use crate::prompt::prompt_builder::{build_observer_prompt, build_player_prompt};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{sleep, Duration};

#[derive(Clone)]
pub struct TableStore {
    tables: Arc<Mutex<HashMap<String, Arc<TableMutex>>>>,
}

type TableMutex = Mutex<TableInner>;

struct LogEntry {
    transition: StateTransition,
    /// `expect` after applying this transition (per design: client applies delta then reads expect).
    expect_after: Expect,
}

struct TableInner {
    state: TableRuntimeState,
    /// `log[i]` has `transition.seq == i + 1`.
    log: Vec<LogEntry>,
    /// Shared so `nextstate` can await notifications without holding the table mutex.
    notify: Arc<Notify>,
}

impl TableStore {
    pub fn new() -> Self {
        Self {
            tables: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for TableStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TableStore {

    pub async fn create_table(&self, table_name: Option<String>) -> TableRuntimeState {
        let id = format!("t_{}", uuid::Uuid::new_v4());
        let state = TableRuntimeState::new(id.clone(), table_name);
        let inner = Arc::new(Mutex::new(TableInner {
            state,
            log: Vec::new(),
            notify: Arc::new(Notify::new()),
        }));
        let mut g = self.tables.lock().await;
        g.insert(id.clone(), inner);
        drop(g);
        self.get_snapshot(&id).await.expect("just inserted")
    }

    pub async fn get_snapshot(&self, table_id: &str) -> Result<TableRuntimeState, AppError> {
        let g = self.tables.lock().await;
        let t = g
            .get(table_id)
            .ok_or_else(|| AppError::NotFound(format!("unknown table_id {}", table_id)))?;
        let inner = t.lock().await;
        Ok(inner.state.clone())
    }

    /// All tables (materialized runtime state), sorted by `table_id` for stable output.
    pub async fn list_table_runtimes(&self) -> Vec<TableRuntimeState> {
        let arcs: Vec<Arc<TableMutex>> = {
            let g = self.tables.lock().await;
            g.values().cloned().collect()
        };
        let mut out = Vec::with_capacity(arcs.len());
        for arc in arcs {
            let inner = arc.lock().await;
            out.push(inner.state.clone());
        }
        out.sort_by(|a, b| a.table_id.cmp(&b.table_id));
        out
    }

    fn pick_seat(state: &TableRuntimeState, requested: SeatOrAuto) -> Result<Seat, AppError> {
        match requested {
            SeatOrAuto::Auto => Seat::ALL
                .into_iter()
                .find(|s| state.seats.get(s).and_then(|x| x.as_ref()).is_none())
                .ok_or_else(|| {
                    AppError::Conflict {
                        message: "table is full".into(),
                        code: "TABLE_FULL",
                        current_seq: Some(state.seq),
                    }
                }),
            SeatOrAuto::Fixed(seat) => {
                if state.seats.get(&seat).and_then(|x| x.as_ref()).is_some() {
                    return Err(AppError::Conflict {
                        message: format!("seat {} is occupied", seat.as_str()),
                        code: "SEAT_TAKEN",
                        current_seq: Some(state.seq),
                    });
                }
                Ok(seat)
            }
        }
    }

    pub async fn join(
        &self,
        table_id: &str,
        player_name: String,
        player_type: Option<PlayerType>,
        seat: SeatOrAuto,
    ) -> Result<(String, Seat, PlayerType), AppError> {
        let arc = {
            let g = self.tables.lock().await;
            g.get(table_id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("unknown table_id {}", table_id)))?
        };

        let mut inner = arc.lock().await;
        if !matches!(inner.state.status, TableStatus::Waiting) {
            return Err(AppError::Conflict {
                message: "cannot join: game already started or finished".into(),
                code: "INVALID_TABLE_STATUS",
                current_seq: Some(inner.state.seq),
            });
        }

        let seat = Self::pick_seat(&inner.state, seat)?;
        let pt = player_type.unwrap_or_default();
        let pid = format!("p_{}", uuid::Uuid::new_v4());
        let prev_snapshot = inner.state.to_table_state();
        let prev_seq = inner.state.seq;

        inner.state.seats.insert(
            seat,
            Some(PlayerRecord {
                player_id: pid.clone(),
                player_name,
                player_type: pt.clone(),
                ready: false,
            }),
        );
        inner.state.seq += 1;
        let new_snapshot = inner.state.to_table_state();
        let seq = inner.state.seq;
        let expect_after = new_snapshot.expect.clone();

        let tr = build_transition(
            &prev_snapshot,
            &new_snapshot,
            prev_seq,
            seq,
            "PLAYER_JOINED",
            Some(json!({
                "actionType": "join",
                "actorPlayerId": pid,
                "seat": seat.as_str(),
            })),
        );
        inner.log.push(LogEntry {
            transition: tr,
            expect_after,
        });
        inner.notify.notify_waiters();

        Ok((pid, seat, pt))
    }

    pub async fn set_ready(
        &self,
        table_id: &str,
        player_id: &str,
        ready: bool,
    ) -> Result<u64, AppError> {
        let arc = {
            let g = self.tables.lock().await;
            g.get(table_id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("unknown table_id {}", table_id)))?
        };

        let mut inner = arc.lock().await;

        let mut found = None;
        for (seat, slot) in &inner.state.seats {
            if let Some(p) = slot.as_ref()
                && p.player_id == player_id
            {
                found = Some((*seat, p.ready));
                break;
            }
        }

        let Some((_seat, was_ready)) = found else {
            return Err(AppError::Forbidden(
                "player is not seated at this table".into(),
            ));
        };

        // Idempotent: no transition or notify.
        if was_ready == ready {
            return Ok(inner.state.seq);
        }

        for (_seat, slot) in &mut inner.state.seats {
            if let Some(p) = slot.as_mut()
                && p.player_id == player_id
            {
                p.ready = ready;
                break;
            }
        }

        let prev_snapshot = inner.state.to_table_state();
        let prev_seq = inner.state.seq;

        let will_start_first_hand =
            inner.state.all_ready() && matches!(inner.state.status, TableStatus::Waiting);
        let will_start_next_hand = inner.state.all_ready()
            && matches!(inner.state.status, TableStatus::InGame)
            && inner.state.waiting_next_hand_ready;
        if will_start_first_hand {
            inner.state.status = TableStatus::InGame;
            inner.state.game_config = GameConfig {
                rng_seed: TableRuntimeState::hash_table_id_seed(&inner.state.table_id),
            };
            let engine = GameEngine::new(inner.state.game_config.clone());
            let mut gs = engine.init_table(inner.state.table_id.clone());
            engine
                .start_first_hand(&mut gs, Seat::E)
                .expect("start_first_hand should not fail");
            inner.state.game = Some(gs);
            inner.state.sync_phase_from_game();
            inner.state.waiting_next_hand_ready = false;
            inner.state.narration.clear();
        } else if will_start_next_hand {
            let seq = inner.state.seq;
            let declarer = inner.state.current_declarer;
            let next_hand_level = level_to_hand_level(match declarer {
                TeamId::Ew => inner.state.team_progress_ew.level,
                TeamId::Sn => inner.state.team_progress_sn.level,
            });
            let finishing_order = inner.state.last_finishing_order.clone();
            let engine = GameEngine::new(inner.state.game_config.clone());
            let canceled_opening_lead = {
                let game = inner
                    .state
                    .game
                    .as_mut()
                    .ok_or_else(|| AppError::Conflict {
                        message: "game state not initialized".into(),
                        code: "INVALID_TABLE_STATUS",
                        current_seq: Some(seq),
                    })?;
                engine
                    .start_next_hand_with_tribute(
                        game,
                        declarer,
                        next_hand_level,
                        &finishing_order,
                    )
                    .map_err(|msg| map_engine_error(msg, seq))?;
                game.hand
                    .as_ref()
                    .and_then(|h| h.tribute.as_ref())
                    .and_then(|t| if t.canceled { Some(game.turn_seat) } else { None })
            };
            inner.state.sync_phase_from_game();
            inner.state.waiting_next_hand_ready = false;
            if let Some(lead) = canceled_opening_lead {
                inner.state.narration =
                    format_tribute_canceled(&player_name_for_seat(&inner.state, lead));
            } else {
                inner.state.narration.clear();
            }
        }

        inner.state.seq += 1;
        let new_snapshot = inner.state.to_table_state();
        let new_seq = inner.state.seq;
        let expect_after = new_snapshot.expect.clone();

        let transition_type = if will_start_first_hand {
            "GAME_STARTED"
        } else if will_start_next_hand {
            "NEXT_HAND_STARTED"
        } else {
            "PLAYER_READY_CHANGED"
        };

        let tr = build_transition(
            &prev_snapshot,
            &new_snapshot,
            prev_seq,
            new_seq,
            transition_type,
            Some(json!({
                "actionType": "ready",
                "actorPlayerId": player_id,
                "ready": ready,
                "gameStarted": will_start_first_hand,
                "nextHandStarted": will_start_next_hand,
            })),
        );
        inner.log.push(LogEntry {
            transition: tr,
            expect_after,
        });

        // If we auto-started game, we still only emitted one transition (merged).
        inner.notify.notify_waiters();

        Ok(new_seq)
    }

    fn apply_action_locked(
        inner: &mut TableInner,
        player_id: &str,
        client_seq: u64,
        action_type: &'static str,
        event_payload: serde_json::Value,
    ) -> Result<u64, AppError> {
        if client_seq != inner.state.seq {
            return Err(AppError::Conflict {
                message: format!(
                    "stale seq: expected {}, got {}",
                    inner.state.seq, client_seq
                ),
                code: "STALE_SEQ",
                current_seq: Some(inner.state.seq),
            });
        }
        if !matches!(inner.state.status, TableStatus::InGame) {
            return Err(AppError::Conflict {
                message: "action is only allowed when table is in_game".into(),
                code: "INVALID_TABLE_STATUS",
                current_seq: Some(inner.state.seq),
            });
        }
        let seat = inner
            .state
            .seat_for_player(player_id)
            .ok_or_else(|| AppError::Forbidden("player is not seated at this table".into()))?;

        let action = parse_player_action(action_type, &event_payload)?;

        let prev_snapshot = inner.state.to_table_state();
        let prev_game = inner.state.game.clone();
        let prev_seq = inner.state.seq;
        let seq = inner.state.seq;
        let playing_commit = matches!(
            inner.state.game.as_ref().map(|g| g.phase),
            Some(GamePhase::Playing)
        )
        .then(|| HandCommitMeta {
            seq: seq + 1,
            timestamp: iso_timestamp(),
        });

        let engine = GameEngine::new(inner.state.game_config.clone());
        let game = inner
            .state
            .game
            .as_mut()
            .ok_or_else(|| AppError::Conflict {
                message: "game state not initialized".into(),
                code: "INVALID_TABLE_STATUS",
                current_seq: Some(seq),
            })?;
        engine
            .apply_player_action(game, seat, action, playing_commit)
            .map_err(|msg| map_engine_error(msg, seq))?;
        inner.state.sync_phase_from_game();
        inner.state.narration = build_action_narration(&inner.state, prev_game.as_ref(), action_type);

        inner.state.seq += 1;
        let new_snapshot = inner.state.to_table_state();
        let new_seq = inner.state.seq;
        let expect_after = new_snapshot.expect.clone();

        let logged_payload = normalized_event_payload(action_type, &event_payload, inner.state.game.as_ref(), new_seq);
        let tr = build_transition(
            &prev_snapshot,
            &new_snapshot,
            prev_seq,
            new_seq,
            "ACTION_APPLIED",
            Some(json!({
                "actionType": action_type,
                "actorPlayerId": player_id,
                "payload": logged_payload
            })),
        );
        inner.log.push(LogEntry {
            transition: tr,
            expect_after,
        });
        inner.notify.notify_waiters();

        // If hand enters scoring, apply scoring and switch to re-ready flow.
        Self::settle_scoring_and_wait_ready(inner)?;

        Ok(inner.state.seq)
    }

    fn settle_scoring_and_wait_ready(inner: &mut TableInner) -> Result<(), AppError> {
        if !matches!(inner.state.status, TableStatus::InGame) {
            return Ok(());
        }

        let seq = inner.state.seq;
        if inner.state.waiting_next_hand_ready {
            return Ok(());
        }
        let game = match inner.state.game.as_ref() {
            Some(g) => g,
            None => {
                return Ok(());
            }
        };
        if game.phase != GamePhase::Scoring {
            return Ok(());
        }
        let winner = game.winner_team.ok_or_else(|| AppError::Conflict {
            message: "winner team missing when entering scoring".into(),
            code: "INVALID_TABLE_STATUS",
            current_seq: Some(seq),
        })?;
        let completed_order = {
            let hand = game
                .hand
                .as_ref()
                .ok_or_else(|| AppError::Conflict {
                    message: "hand missing when entering scoring".into(),
                    code: "INVALID_TABLE_STATUS",
                    current_seq: Some(seq),
                })?;
            complete_finishing_order(hand)
        };

        let prev_snapshot = inner.state.to_table_state();
        let prev_seq = inner.state.seq;
        let win_type = infer_win_type(&completed_order, winner);
        let outcome = ScoringService::apply_hand(
            inner.state.team_progress_ew.clone(),
            inner.state.team_progress_sn.clone(),
            inner.state.current_declarer,
            winner,
            win_type,
            true,
        )
        .map_err(|msg| AppError::Conflict {
            message: msg,
            code: "INVALID_SCORING",
            current_seq: Some(seq),
        })?;

        inner.state.team_progress_ew = outcome.progress_ew;
        inner.state.team_progress_sn = outcome.progress_sn;
        inner.state.current_declarer = outcome.next_declarer;
        inner.state.last_finishing_order = completed_order.clone();
        reset_all_players_ready(&mut inner.state);

        let ew_level = level_to_api(inner.state.team_progress_ew.level);
        let sn_level = level_to_api(inner.state.team_progress_sn.level);
        let finish_names = completed_order
            .iter()
            .map(|seat| player_name_for_seat(&inner.state, *seat))
            .collect::<Vec<_>>();
        if outcome.winner_team.is_some() {
            inner.state.status = TableStatus::Finished;
            inner.state.waiting_next_hand_ready = false;
            inner.state.narration = format_hand_end(&finish_names, ew_level, sn_level, false, true);
            if let Some(g) = inner.state.game.as_mut() {
                g.phase = GamePhase::Completed;
            }
            inner.state.sync_phase_from_game();
        } else {
            inner.state.waiting_next_hand_ready = true;
            inner.state.narration = format_hand_end(&finish_names, ew_level, sn_level, true, false);
        }

        inner.state.seq += 1;
        let new_snapshot = inner.state.to_table_state();
        let new_seq = inner.state.seq;
        let expect_after = new_snapshot.expect.clone();

        let transition_type = if matches!(inner.state.status, TableStatus::Finished) {
            "GAME_COMPLETED"
        } else {
            "HAND_ENDED_WAITING_READY"
        };
        let tr = build_transition(
            &prev_snapshot,
            &new_snapshot,
            prev_seq,
            new_seq,
            transition_type,
            Some(json!({
                "actionType": "hand_end",
                "winnerTeamId": winner.as_str(),
                "finishingOrder": completed_order.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            })),
        );
        inner.log.push(LogEntry {
            transition: tr,
            expect_after,
        });
        inner.notify.notify_waiters();
        Ok(())
    }

    pub async fn apply_action(
        &self,
        table_id: &str,
        player_id: &str,
        client_seq: u64,
        action_type: &'static str,
        event_payload: serde_json::Value,
    ) -> Result<u64, AppError> {
        let arc = {
            let g = self.tables.lock().await;
            g.get(table_id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("unknown table_id {}", table_id)))?
        };
        let mut inner = arc.lock().await;
        Self::apply_action_locked(
            &mut inner,
            player_id,
            client_seq,
            action_type,
            event_payload,
        )
    }

    /// Fetch transition `since_seq + 1`, waiting if needed. Returns `None` on timeout (204).
    pub async fn next_state(
        &self,
        table_id: &str,
        since_seq: u64,
        timeout: Option<Duration>,
    ) -> Result<Option<NextStateBody>, AppError> {
        let arc = {
            let g = self.tables.lock().await;
            g.get(table_id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("unknown table_id {}", table_id)))?
        };

        let timeout = timeout.unwrap_or(Duration::from_secs(60));

        loop {
            let notify = {
                let inner = arc.lock().await;
                if since_seq > inner.state.seq {
                    return Err(AppError::BadRequest(format!(
                        "sinceSeq {} is ahead of currentSeq {}",
                        since_seq, inner.state.seq
                    )));
                }
                if since_seq < inner.state.seq {
                    let idx = since_seq as usize;
                    let entry = inner.log.get(idx).ok_or_else(|| {
                        AppError::BadRequest("internal: missing transition for seq".into())
                    })?;
                    let tr = entry.transition.clone();
                    let tr_seq = tr.seq;
                    let expect = entry.expect_after.clone();
                    let prompt = None;
                    let lag = inner
                        .state
                        .seq
                        .saturating_sub(tr_seq);
                    return Ok(Some(NextStateBody {
                        transition: tr,
                        lag,
                        expect,
                        private: None,
                        prompt,
                    }));
                }
                // since_seq == current: subscribe then release lock before awaiting.
                inner.notify.clone()
            };

            let wait = notify.notified();

            tokio::select! {
                _ = wait => { continue; }
                _ = sleep(timeout) => { return Ok(None); }
            }
        }
    }

    pub async fn next_state_with_prompt(
        &self,
        table_id: &str,
        since_seq: u64,
        player_id: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<Option<NextStateBody>, AppError> {
        let body = self.next_state(table_id, since_seq, timeout).await?;
        let Some(mut body) = body else { return Ok(None); };

        if let Some(pid) = player_id {
            let snap = self.get_snapshot(table_id).await?;
            let mine_ready = snap
                .seats
                .values()
                .flatten()
                .find(|p| p.player_id == pid)
                .map(|p| p.ready);
            let is_actor = body.expect.actor_player_id.as_deref() == Some(pid);
            body.prompt = Some(build_player_prompt(&body.expect, mine_ready, is_actor));
            body.private = snap.private_view_for_player(pid);
        } else {
            // Observer: read-only prompt
            body.prompt = Some(build_observer_prompt(&body.expect));
        }

        Ok(Some(body))
    }
}

fn build_action_narration(
    state: &TableRuntimeState,
    prev_game: Option<&crate::game::types::TableGameState>,
    action_type: &'static str,
) -> String {
    if action_type == "play" {
        return build_play_narration(state, prev_game);
    }
    let Some(game) = state.game.as_ref() else {
        return String::new();
    };
    let Some(hand) = game.hand.as_ref() else {
        return String::new();
    };
    let Some(tribute) = hand.tribute.as_ref() else {
        return String::new();
    };
    let prev_pairs = prev_game
        .and_then(|g| g.hand.as_ref())
        .and_then(|h| h.tribute.as_ref())
        .map(|t| &t.pairs);

    for pair in &tribute.pairs {
        let prev_pair = prev_pairs.and_then(|pairs| {
            pairs
                .iter()
                .find(|x| x.payer == pair.payer && x.receiver == pair.receiver)
        });
        if action_type == "tribute" {
            let changed = pair.paid_card.is_some()
                && prev_pair.and_then(|p| p.paid_card.as_deref()) != pair.paid_card.as_deref();
            if changed
                && let Some(card) = pair.paid_card.as_deref()
            {
                return format_tribute_action(
                    &player_name_for_seat(state, pair.payer),
                    card,
                    &player_name_for_seat(state, pair.receiver),
                    false,
                );
            }
        } else if action_type == "return_card" {
            let changed = pair.return_card.is_some()
                && prev_pair.and_then(|p| p.return_card.as_deref()) != pair.return_card.as_deref();
            if changed
                && let Some(card) = pair.return_card.as_deref()
            {
                return format_tribute_action(
                    &player_name_for_seat(state, pair.receiver),
                    card,
                    &player_name_for_seat(state, pair.payer),
                    true,
                );
            }
        }
    }
    String::new()
}

fn build_play_narration(
    state: &TableRuntimeState,
    prev_game: Option<&crate::game::types::TableGameState>,
) -> String {
    let Some(game) = state.game.as_ref() else {
        return String::new();
    };
    let Some(hand) = game.hand.as_ref() else {
        return String::new();
    };
    let Some(last) = hand.history.last() else {
        return String::new();
    };
    if last.action_type != HistoryActionKind::Play {
        return String::new();
    }
    let Some(comb) = last.combination_type.as_deref() else {
        return String::new();
    };
    if !is_big_play_combination(comb) {
        let prev_finishing_len = prev_game
            .and_then(|g| g.hand.as_ref())
            .map(|h| h.finishing_order.len())
            .unwrap_or(0);
        if hand.finishing_order.len() > prev_finishing_len {
            let rank = hand.finishing_order.len();
            if rank <= 2
                && let Some(seat) = hand.finishing_order.last().copied()
            {
                return format_rank_announce(&player_name_for_seat(state, seat), rank);
            }
        }
        return String::new();
    }
    format_big_play(&player_name_for_seat(state, last.seat), comb)
}

fn player_name_for_seat(state: &TableRuntimeState, seat: Seat) -> String {
    state
        .seats
        .get(&seat)
        .and_then(|o| o.as_ref())
        .map(|p| p.player_name.clone())
        .unwrap_or_else(|| seat.as_str().to_string())
}

fn reset_all_players_ready(state: &mut TableRuntimeState) {
    for slot in state.seats.values_mut() {
        if let Some(p) = slot.as_mut() {
            p.ready = false;
        }
    }
}

fn complete_finishing_order(hand: &HandState) -> Vec<Seat> {
    let mut order: Vec<Seat> = hand.finishing_order.clone();
    for seat in Seat::ALL {
        if !order.contains(&seat) {
            order.push(seat);
        }
    }
    order
}

fn seat_team(seat: Seat) -> TeamId {
    match seat {
        Seat::E | Seat::W => TeamId::Ew,
        Seat::S | Seat::N => TeamId::Sn,
    }
}

fn infer_win_type(order: &[Seat], winner: TeamId) -> WinType {
    if order.len() >= 2 && seat_team(order[0]) == winner && seat_team(order[1]) == winner {
        return WinType::OneTwo;
    }
    if order.len() >= 3 && seat_team(order[0]) == winner && seat_team(order[2]) == winner {
        return WinType::OneThree;
    }
    WinType::OneFour
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

fn level_to_hand_level(level: Level) -> HandLevel {
    match level {
        Level::Two => HandLevel::Two,
        Level::Three => HandLevel::Three,
        Level::Four => HandLevel::Four,
        Level::Five => HandLevel::Five,
        Level::Six => HandLevel::Six,
        Level::Seven => HandLevel::Seven,
        Level::Eight => HandLevel::Eight,
        Level::Nine => HandLevel::Nine,
        Level::Ten => HandLevel::Ten,
        Level::J => HandLevel::J,
        Level::Q => HandLevel::Q,
        Level::K => HandLevel::K,
        Level::A => HandLevel::A,
    }
}

#[derive(Clone, Copy)]
pub enum SeatOrAuto {
    Auto,
    Fixed(Seat),
}

impl SeatOrAuto {
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "auto" => Ok(SeatOrAuto::Auto),
            "E" => Ok(SeatOrAuto::Fixed(Seat::E)),
            "S" => Ok(SeatOrAuto::Fixed(Seat::S)),
            "W" => Ok(SeatOrAuto::Fixed(Seat::W)),
            "N" => Ok(SeatOrAuto::Fixed(Seat::N)),
            _ => Err(AppError::BadRequest(format!("invalid seat {:?}", s))),
        }
    }
}

fn map_engine_error(message: String, current_seq: u64) -> AppError {
    let code: &'static str = if message.contains("wrong turn") {
        "WRONG_TURN"
    } else if message.contains("not allowed in current phase")
        || message.contains("not expected to tribute")
        || message.contains("not expected to return")
    {
        "INVALID_PHASE_ACTION"
    } else {
        "ILLEGAL_ACTION"
    };
    AppError::Unprocessable {
        message,
        code,
        current_seq: Some(current_seq),
    }
}

fn parse_player_action(
    action_type: &'static str,
    payload: &serde_json::Value,
) -> Result<PlayerAction, AppError> {
    match action_type {
        "tribute" => {
            let card = payload
                .get("card")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::BadRequest("missing card".into()))?
                .to_string();
            Ok(PlayerAction::Tribute { card })
        }
        "return_card" => {
            let card = payload
                .get("card")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::BadRequest("missing card".into()))?
                .to_string();
            Ok(PlayerAction::ReturnCard { card })
        }
        "play" => {
            let cards: Vec<String> = serde_json::from_value(
                payload
                    .get("cards")
                    .cloned()
                    .ok_or_else(|| AppError::BadRequest("missing cards".into()))?,
            )
            .map_err(|e| AppError::BadRequest(format!("cards: {}", e)))?;
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
        _ => Err(AppError::BadRequest("unknown action_type".into())),
    }
}

fn normalized_event_payload(
    action_type: &'static str,
    event_payload: &serde_json::Value,
    game: Option<&crate::game::types::TableGameState>,
    new_seq: u64,
) -> serde_json::Value {
    if action_type != "play" {
        return event_payload.clone();
    }
    let Some(g) = game else {
        return event_payload.clone();
    };
    let Some(hand) = g.hand.as_ref() else {
        return event_payload.clone();
    };
    let Some(last) = hand.history.last() else {
        return event_payload.clone();
    };
    if last.seq != new_seq || last.action_type != crate::game::types::HistoryActionKind::Play {
        return event_payload.clone();
    }
    let mut payload = json!({
        "cards": last.cards.clone(),
    });
    if let Some(wt) = &last.wild_targets {
        payload["declaredWildMapping"] = json!({ "wildTargets": wt });
    }
    payload
}

fn build_transition(
    prev: &TableState,
    next: &TableState,
    prev_seq: u64,
    seq: u64,
    transition_type: &str,
    event: Option<serde_json::Value>,
) -> StateTransition {
    let mut delta = snapshot_replace_delta(prev, next);
    delta.event = event.map(|trigger| json!({ "trigger": trigger, "derived": [] }));
    StateTransition {
        seq,
        prev_seq,
        table_id: next.table_id.clone(),
        timestamp: iso_timestamp(),
        transition_type: transition_type.into(),
        delta,
    }
}

#[cfg(feature = "test-utils")]
impl TableStore {
    /// Replace in-memory engine state (preserves `seq` and transition log).
    /// Hidden hook for integration tests; not part of the public HTTP contract.
    #[doc(hidden)]
    pub async fn test_set_game_state(
        &self,
        table_id: &str,
        game: crate::game::types::TableGameState,
        game_config: GameConfig,
    ) -> Result<(), AppError> {
        let arc = {
            let g = self.tables.lock().await;
            g.get(table_id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("unknown table_id {}", table_id)))?
        };
        let mut inner = arc.lock().await;
        inner.state.game_config = game_config;
        inner.state.game = Some(game);
        inner.state.sync_phase_from_game();
        inner.state.status = TableStatus::InGame;
        inner.state.waiting_next_hand_ready = false;
        inner.state.narration.clear();
        Ok(())
    }
}
