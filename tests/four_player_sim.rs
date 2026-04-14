//! Engine + HTTP simulation smoke tests.

use clawguandan::domain::Seat;
use clawguandan::game::card::HandLevel;
use clawguandan::game::engine::GameEngine;
use clawguandan::game::rules::scoring::Level;
use clawguandan::game::types::{GameConfig, GamePhase, HandState, TableGameState, TeamId};
use clawguandan::simulation::{
    cli_argv_table_create, run_cli_command, run_hand_until_scoring_via_router, run_match_engine,
};
use clawguandan::store::{SeatOrAuto, TableStore};
use clawguandan::strategy::enumerate_legal_actions;

#[test]
fn movegen_fixture_endgame_has_lead() {
    use clawguandan::game::test_support::TestFixtures;
    let s = TestFixtures::table_game_playing_four_singles_endgame();
    let legal = enumerate_legal_actions(&s, Seat::E).expect("legal");
    assert!(
        legal
            .iter()
            .any(|a| matches!(a, clawguandan::game::engine::PlayerAction::Play { .. })),
        "expected a play option for leader"
    );
}

#[test]
fn engine_suggest_play_one_hand_reaches_scoring() {
    let eng = GameEngine::new(GameConfig { rng_seed: 42 });
    let mut state = TableGameState::new("t_sim".into());
    eng.start_first_hand(&mut state, Seat::E, HandLevel::Two)
        .expect("deal");
    assert_eq!(state.phase, GamePhase::Playing);

    let out = run_match_engine(&eng, &mut state, 1, 100_000).expect("sim");
    assert_eq!(out.hands_played, 1);
    assert_eq!(state.phase, GamePhase::Scoring);
}

/// [`GameEngine::start_next_hand_with_tribute`] needs a four-seat finishing order; movegen should list tribute candidates.
#[test]
fn second_hand_enters_tribute_after_synthetic_finishing_order() {
    let eng = GameEngine::new(GameConfig { rng_seed: 5 });
    let mut state = TableGameState::new("t_syn".into());
    state.phase = GamePhase::Scoring;
    let mut hand = HandState::new(clawguandan::game::card::HandLevel::Two);
    hand.finishing_order = vec![Seat::E, Seat::S, Seat::W, Seat::N];
    for s in Seat::ALL {
        hand.hands.insert(s, vec![]);
    }
    state.hand = Some(hand);
    state.winner_team = Some(TeamId::Ew);
    eng.start_next_hand_with_tribute(
        &mut state,
        TeamId::Ew,
        HandLevel::Two,
        &[Seat::E, Seat::S, Seat::W, Seat::N],
    )
    .expect("next hand");
    assert_eq!(state.phase, GamePhase::Tribute);
    let actor = clawguandan::strategy::current_actor_seat(&state).expect("tribute actor");
    let legal = enumerate_legal_actions(&state, actor).expect("legal");
    assert!(
        legal
            .iter()
            .any(|a| matches!(a, clawguandan::game::engine::PlayerAction::Tribute { .. })),
        "expected tribute options"
    );
}

#[test]
fn cli_binary_help_smoke() {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_clawguandan"));
    run_cli_command(bin, &["--help"]).expect("cli --help");
}

#[test]
fn cli_argv_table_create_matches_clap() {
    let v = cli_argv_table_create(Some("lobby"), Some("8"));
    assert_eq!(
        v,
        vec!["table", "create", "lobby", "--rank", "8"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn http_router_hand_until_scoring_smoke() {
    use clawguandan::api::app_with_store;

    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let t = store.create_table(None, Level::Two).await;
    let table_id = t.table_id.clone();

    let mut pids = Vec::new();
    for _ in 0..4 {
        let (pid, _, _) = store
            .join(&table_id, "p".into(), None, SeatOrAuto::Auto)
            .await
            .unwrap();
        pids.push(pid);
    }

    for pid in &pids {
        store.set_ready(&table_id, pid, true).await.unwrap();
    }

    run_hand_until_scoring_via_router(app, &store, &table_id, 150_000)
        .await
        .expect("router sim");

    let snap = store.get_snapshot(&table_id).await.unwrap();
    let g = snap.game.expect("game");
    assert_eq!(g.phase, GamePhase::Scoring);
}
