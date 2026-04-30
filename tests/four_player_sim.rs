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
use std::collections::HashMap;

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
///
/// Tribute can be **canceled** (抗贡) when payers collectively hold enough red jokers; that skips `Tribute` and opens in
/// `Playing`. This test searches a small RNG seed range until the deal does not cancel, so assertions stay stable.
#[test]
fn second_hand_enters_tribute_after_synthetic_finishing_order() {
    const FINISHING: [Seat; 4] = [Seat::E, Seat::S, Seat::W, Seat::N];
    let mut state = None;
    for seed in 0u64..500 {
        let eng = GameEngine::new(GameConfig { rng_seed: seed });
        let mut st = TableGameState::new("t_syn".into());
        st.phase = GamePhase::Scoring;
        let mut hand = HandState::new(clawguandan::game::card::HandLevel::Two);
        hand.finishing_order = FINISHING.to_vec();
        for s in Seat::ALL {
            hand.hands.insert(s, vec![]);
        }
        st.hand = Some(hand);
        st.winner_team = Some(TeamId::Ew);
        if eng
            .start_next_hand_with_tribute(&mut st, TeamId::Ew, HandLevel::Two, &FINISHING)
            .is_err()
        {
            continue;
        }
        if st.phase == GamePhase::Tribute {
            state = Some(st);
            break;
        }
    }
    let state = state.expect("expected some rng_seed < 500 to deal a non-canceled tribute");
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
fn cli_show_help_lists_version_subcommand() {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_clawguandan"));
    let out = run_cli_command(bin, &["show", "--help"]).expect("show --help");
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(
        help.contains("version"),
        "expected `show --help` to include `version`, got:\n{}",
        help
    );
}

#[test]
fn cli_show_version_prints_layered_version_info() {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_clawguandan"));
    let out = run_cli_command(bin, &["show", "version"]).expect("show version");
    let got = String::from_utf8_lossy(&out.stdout);
    assert!(got.contains(&format!("name: {}", env!("CARGO_PKG_NAME"))));
    assert!(got.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))));
    assert!(got.contains(&format!(
        "same_as_--version: {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )));
}

#[test]
fn cli_show_version_json_outputs_structured_payload() {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_clawguandan"));
    let out = run_cli_command(bin, &["show", "version", "--json"]).expect("show version --json");
    let got = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&got).expect("valid json");
    assert_eq!(
        v.get("name").and_then(|x| x.as_str()),
        Some(env!("CARGO_PKG_NAME"))
    );
    assert_eq!(
        v.get("version").and_then(|x| x.as_str()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    let expected_same = format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    assert_eq!(
        v.get("sameAsVersionFlag").and_then(|x| x.as_str()),
        Some(expected_same.as_str())
    );
    assert!(v.get("target").and_then(|x| x.as_str()).is_some());
}

#[test]
fn cli_show_verion_alias_still_works() {
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_clawguandan"));
    let out = run_cli_command(bin, &["show", "verion"]).expect("show verion");
    let got = String::from_utf8_lossy(&out.stdout);
    assert!(got.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))));
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
    let mut player_keys: HashMap<String, String> = HashMap::new();
    for _ in 0..4 {
        let (pid, pkey, _, _, _) = store
            .join(&table_id, "p".into(), None, None, SeatOrAuto::Auto)
            .await
            .unwrap();
        player_keys.insert(pid.clone(), pkey);
        pids.push(pid);
    }

    for pid in &pids {
        let pkey = player_keys.get(pid).expect("player key");
        store.set_ready(&table_id, pid, pkey, true).await.unwrap();
    }

    run_hand_until_scoring_via_router(app, &store, &table_id, &player_keys, 150_000)
        .await
        .expect("router sim");

    let snap = store.get_snapshot(&table_id).await.unwrap();
    let g = snap.game.expect("game");
    assert_eq!(g.phase, GamePhase::Scoring);
}
