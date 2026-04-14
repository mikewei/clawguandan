#![cfg(feature = "test-utils")]
//! HTTP 集成：注入四人各一张终局牌面，按固定顺序 play/pass 直至本手进入 `scoring`。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use clawguandan::api::app_with_store;
use clawguandan::domain::Seat;
use clawguandan::game::test_support::TestFixtures;
use clawguandan::game::types::{GameConfig, GamePhase};
use clawguandan::store::TableStore;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn read_json(res: axum::response::Response) -> Value {
    let body = res.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_ready_table(app: axum::Router) -> (axum::Router, String, Vec<String>, u64) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tables")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = read_json(res).await;
    let table_id = v["tableId"].as_str().unwrap().to_string();
    let mut pids = Vec::new();
    for i in 0..4 {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/tables/{}/join", table_id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "playerName": format!("P{}", i),
                            "seat": "auto",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = read_json(res).await;
        pids.push(body["playerId"].as_str().unwrap().to_string());
    }
    let mut seq = 0u64;
    for pid in &pids {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/tables/{}/ready", table_id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"playerId": pid, "ready": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = read_json(res).await;
        seq = body["newSeq"].as_u64().unwrap();
    }
    (app, table_id, pids, seq)
}

#[tokio::test]
async fn full_hand_four_players_until_scoring() {
    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let (app, table_id, pids, seq) = create_ready_table(app).await;

    let snap = store.get_snapshot(&table_id).await.unwrap();
    store
        .test_set_game_state(
            &table_id,
            TestFixtures::table_game_playing_four_singles_endgame(),
            GameConfig {
                rng_seed: snap.game_config.rng_seed,
            },
        )
        .await
        .unwrap();

    // Seats: E,S,W,N == pids[0..4].
    let e = &pids[0];

    // Build a deterministic scoring state: E has one card, W already empty.
    // E plays out -> EW both empty -> scoring.
    let mut game = TestFixtures::table_game_playing_four_singles_endgame();
    game.phase = GamePhase::Playing;
    game.turn_seat = Seat::E;
    game.leader_seat = Seat::E;
    let hand = game.hand.as_mut().unwrap();
    hand.finishing_order.clear();
    hand.history.clear();
    hand.trick.top_play = None;
    hand.trick.last_play_seat = None;
    hand.trick.consecutive_passes = 0;
    hand.hands.insert(Seat::E, vec!["♠3".into()]);
    hand.hands.insert(Seat::W, vec![]);
    hand.hands.insert(Seat::S, vec!["♠4".into(), "♠5".into()]);
    hand.hands.insert(Seat::N, vec!["♠6".into(), "♠7".into()]);
    store
        .test_set_game_state(
            &table_id,
            game,
            GameConfig {
                rng_seed: snap.game_config.rng_seed,
            },
        )
        .await
        .unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/play", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": e,
                        "seq": seq,
                        "cards": ["♠3"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let mut seq_after_scoring = read_json(res).await["newSeq"].as_u64().unwrap();

    // 本手结束后进入 scoring，且要求四人重新 ready 才开下一手。
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/snapshot?playerId={}",
                    table_id, e
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let snap = read_json(res).await;
    assert_eq!(snap["phase"], "scoring");
    assert_eq!(snap["expect"]["kind"], "ready");
    assert!(
        snap["narration"]
            .as_str()
            .is_some_and(|s| s.contains("请全员再次准备")),
        "expected end-of-hand narration, got {:?}",
        snap["narration"]
    );
    for seat in ["E", "S", "W", "N"] {
        assert_eq!(
            snap["seats"][seat]["ready"], false,
            "seat {seat} should be reset"
        );
    }
    let hist = snap["hand"]["history"].as_array().expect("hand.history");
    assert_eq!(
        hist.len(),
        1,
        "expected a single winning play in hand.history"
    );

    for (idx, pid) in pids.iter().enumerate() {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/tables/{}/ready", table_id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"playerId": pid, "ready": true}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = read_json(res).await;
        seq_after_scoring = body["newSeq"].as_u64().unwrap();
        if idx < 3 {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!(
                            "/api/v1/tables/{}/snapshot?playerId={}",
                            table_id, e
                        ))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let snap_mid = read_json(res).await;
            assert_eq!(snap_mid["expect"]["kind"], "ready");
        }
    }

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/snapshot?playerId={}",
                    table_id, e
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let next_snap = read_json(res).await;
    assert_eq!(next_snap["phase"], "tribute");
    assert_eq!(next_snap["expect"]["kind"], "tribute");
    assert_eq!(next_snap["seq"], json!(seq_after_scoring));
    assert_eq!(next_snap["narration"], "");
}
