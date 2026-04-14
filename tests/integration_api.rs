#![cfg(feature = "test-utils")]
//! HTTP 集成：注入进贡/还牌状态后走完整 action 链，并验证 seq、expect、private。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use clawguandan::api::app_with_store;
use clawguandan::game::card::{
    level_order_value, parse_card_symbol, HandLevel, Suit, RuleContext,
};
use clawguandan::domain::Seat;
use clawguandan::game::rules::combination_parser::CombinationParser;
use clawguandan::game::test_support::TestFixtures;
use clawguandan::game::types::{GameConfig, GamePhase, HistoryActionKind, PlayState};
use clawguandan::store::TableStore;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn read_json(res: axum::response::Response) -> Value {
    let body = res.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn snapshot_player(app: &axum::Router, table_id: &str, pid: &str) -> Value {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/snapshot?playerId={}",
                    table_id, pid
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(res).await
}

fn suit_weight_desc(suit: Suit) -> u8 {
    match suit {
        Suit::Hearts => 4,
        Suit::Spades => 3,
        Suit::Diamonds => 2,
        Suit::Clubs => 1,
        Suit::Joker => 0,
    }
}

fn parse_hand_level_api(level: &str) -> HandLevel {
    match level {
        "2" => HandLevel::Two,
        "3" => HandLevel::Three,
        "4" => HandLevel::Four,
        "5" => HandLevel::Five,
        "6" => HandLevel::Six,
        "7" => HandLevel::Seven,
        "8" => HandLevel::Eight,
        "9" => HandLevel::Nine,
        "10" => HandLevel::Ten,
        "J" => HandLevel::J,
        "Q" => HandLevel::Q,
        "K" => HandLevel::K,
        "A" => HandLevel::A,
        _ => panic!("unknown hand level: {level}"),
    }
}

fn assert_cards_desc(cards: &[Value], hand_level: HandLevel) {
    let ctx = RuleContext { hand_level };
    let parsed: Vec<_> = cards
        .iter()
        .map(|v| v.as_str().expect("hand card should be string"))
        .map(|s| parse_card_symbol(s).expect("hand card should be parseable"))
        .collect();
    for pair in parsed.windows(2) {
        let left = pair[0];
        let right = pair[1];
        let left_key = (level_order_value(left, ctx), suit_weight_desc(left.suit));
        let right_key = (level_order_value(right, ctx), suit_weight_desc(right.suit));
        assert!(
            left_key >= right_key,
            "cards should be sorted desc, got left={:?} right={:?}",
            left,
            right
        );
    }
}

async fn create_ready_table_with_store(app: axum::Router) -> (axum::Router, String, Vec<String>, u64) {
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
async fn ping_returns_pong_and_version() {
    let app = app_with_store(TableStore::new());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v = read_json(res).await;
    assert_eq!(v["pong"], json!("clawguandan"));
    assert!(v.get("ver").and_then(|x| x.as_str()).is_some());
    assert_eq!(
        v["pid"].as_u64(),
        Some(u64::from(std::process::id())),
        "ping should report this process id in tests"
    );
}

#[tokio::test]
async fn embedded_root_serves_index_html() {
    let app = app_with_store(TableStore::new());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|x| x.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/html"),
        "unexpected content-type: {content_type}"
    );
}

#[tokio::test]
async fn embedded_js_asset_is_accessible() {
    let app = app_with_store(TableStore::new());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|x| x.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("application/javascript"),
        "unexpected content-type: {content_type}"
    );
}

#[tokio::test]
async fn injected_tribute_exchange_advances_seq_and_reaches_playing() {
    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let (app, table_id, pids, mut seq) = create_ready_table_with_store(app).await;

    let snap = store.get_snapshot(&table_id).await.unwrap();
    store
        .test_set_game_state(
            &table_id,
            TestFixtures::table_game_tribute_two_pairs(),
            GameConfig {
                rng_seed: snap.game_config.rng_seed,
            },
        )
        .await
        .unwrap();

    let s = snapshot_player(&app, &table_id, &pids[0]).await;
    assert_eq!(s["expect"]["kind"], "tribute");
    let snapshot_cards = s["private"]["handCards"].as_array().unwrap();
    let snapshot_hand_level = parse_hand_level_api(s["hand"]["handLevel"].as_str().unwrap());
    assert!(!snapshot_cards.is_empty());
    assert_cards_desc(snapshot_cards, snapshot_hand_level);

    // Seats: E,S,W,N == pids[0..4]. W pays ♠A, N pays ♦K.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/tribute", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[2], "seq": seq, "card": "♠A"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    seq = read_json(res).await["newSeq"].as_u64().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/tribute", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[3], "seq": seq, "card": "♦K"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    seq = read_json(res).await["newSeq"].as_u64().unwrap();

    let ex = snapshot_player(&app, &table_id, &pids[0]).await;
    assert_eq!(ex["expect"]["kind"], "exchange");

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/return_card", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[0], "seq": seq, "card": "♦5"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    seq = read_json(res).await["newSeq"].as_u64().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/return_card", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[1], "seq": seq, "card": "♠8"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    seq = read_json(res).await["newSeq"].as_u64().unwrap();

    let fin = snapshot_player(&app, &table_id, &pids[0]).await;
    assert_eq!(fin["expect"]["kind"], "play");
    assert_eq!(fin["phase"], "playing");

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=50&playerId={}",
                    table_id,
                    seq - 1,
                    pids[0]
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let tr = read_json(res).await;
    assert_eq!(tr["seq"], seq);
    assert_eq!(tr["type"], "ACTION_APPLIED");
    let nextstate_cards = tr["private"]["handCards"].as_array().unwrap();
    assert!(!nextstate_cards.is_empty());
    assert_cards_desc(nextstate_cards, snapshot_hand_level);
}

#[tokio::test]
async fn nextstate_immediate_catch_up_when_behind() {
    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let (app, table_id, pids, seq) = create_ready_table_with_store(app).await;

    store
        .test_set_game_state(
            &table_id,
            TestFixtures::table_game_tribute_two_pairs(),
            GameConfig {
                rng_seed: store.get_snapshot(&table_id).await.unwrap().game_config.rng_seed,
            },
        )
        .await
        .unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/tribute", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[2], "seq": seq, "card": "♠A"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let new_seq = read_json(res).await["newSeq"].as_u64().unwrap();

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=50",
                    table_id,
                    seq
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = read_json(res).await;
    assert_eq!(body["seq"], new_seq);
    assert_eq!(body["prevSeq"], seq);
}

#[tokio::test]
async fn play_without_declared_mapping_auto_fills_and_logs_mapping() {
    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let (app, table_id, pids, seq) = create_ready_table_with_store(app).await;

    let mut game = TestFixtures::table_game_playing_four_singles_endgame();
    game.phase = GamePhase::Playing;
    game.turn_seat = Seat::E;
    game.leader_seat = Seat::E;
    let hand = game.hand.as_mut().unwrap();
    hand.hand_level = HandLevel::Three;
    hand.hands.insert(Seat::E, vec!["♥3".into(), "♠J".into()]);
    hand.hands.insert(Seat::S, vec!["♠4".into()]);
    hand.hands.insert(Seat::W, vec!["♠5".into()]);
    hand.hands.insert(Seat::N, vec!["♠6".into()]);
    let top_cards = vec!["♠10".to_string(), "♦10".to_string()];
    let top_combo = CombinationParser::parse(
        &top_cards,
        None,
        RuleContext {
            hand_level: hand.hand_level,
        },
    )
    .unwrap();
    hand.trick.top_play = Some(PlayState {
        seat: Seat::N,
        cards: top_cards,
        wild_targets: None,
        combination: top_combo,
    });
    hand.trick.last_play_seat = Some(Seat::N);

    store
        .test_set_game_state(
            &table_id,
            game,
            GameConfig {
                rng_seed: store.get_snapshot(&table_id).await.unwrap().game_config.rng_seed,
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
                        "playerId": &pids[0],
                        "seq": seq,
                        "cards": ["♥3", "♠J"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let new_seq = read_json(res).await["newSeq"].as_u64().unwrap();

    let snap = store.get_snapshot(&table_id).await.unwrap();
    let last = snap
        .game
        .as_ref()
        .unwrap()
        .hand
        .as_ref()
        .unwrap()
        .history
        .last()
        .unwrap();
    assert_eq!(last.action_type, HistoryActionKind::Play);
    assert!(last.wild_targets.as_ref().is_some_and(|wt| !wt.is_empty()));

    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=50",
                    table_id, seq
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let tr = read_json(res).await;
    assert_eq!(tr["seq"], new_seq);
    let wt = tr["delta"]["event"]["trigger"]["payload"]["declaredWildMapping"]["wildTargets"]
        .as_array()
        .unwrap();
    assert!(!wt.is_empty());
}

#[tokio::test]
async fn finishing_player_is_recorded_and_next_actor_skips_empty_seat() {
    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let (app, table_id, pids, seq) = create_ready_table_with_store(app).await;

    let mut game = TestFixtures::table_game_playing_four_singles_endgame();
    game.phase = GamePhase::Playing;
    game.turn_seat = Seat::E;
    game.leader_seat = Seat::E;
    let hand = game.hand.as_mut().unwrap();
    hand.hands.insert(Seat::E, vec!["♠3".into()]);
    hand.hands.insert(Seat::N, vec![]);
    hand.hands.insert(Seat::W, vec!["♠4".into()]);
    hand.hands.insert(Seat::S, vec!["♠5".into()]);

    store
        .test_set_game_state(
            &table_id,
            game,
            GameConfig {
                rng_seed: store.get_snapshot(&table_id).await.unwrap().game_config.rng_seed,
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
                        "playerId": &pids[0],
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

    let snap = snapshot_player(&app, &table_id, &pids[2]).await;
    assert_eq!(snap["expect"]["kind"], "play");
    assert_eq!(
        snap["expect"]["actorPlayerId"].as_str(),
        Some(pids[2].as_str())
    );
    let finishing = snap["hand"]["finishingOrder"]
        .as_array()
        .expect("finishingOrder");
    assert_eq!(finishing.len(), 1);
    assert_eq!(finishing[0], json!("E"));
    assert!(
        snap["narration"]
            .as_str()
            .is_some_and(|s| s.contains("头游")),
        "expected head-rank narration, got {:?}",
        snap["narration"]
    );
}

#[tokio::test]
async fn bomb_play_updates_table_narration() {
    let store = TableStore::new();
    let app = app_with_store(store.clone());
    let (app, table_id, pids, seq) = create_ready_table_with_store(app).await;

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
    hand.hands.insert(
        Seat::E,
        vec!["♠3".into(), "♥3".into(), "♦3".into(), "♣3".into()],
    );
    hand.hands.insert(Seat::S, vec!["♠4".into(), "♠5".into()]);
    hand.hands.insert(Seat::W, vec!["♠6".into(), "♠7".into()]);
    hand.hands.insert(Seat::N, vec!["♠8".into(), "♠9".into()]);

    store
        .test_set_game_state(
            &table_id,
            game,
            GameConfig {
                rng_seed: store.get_snapshot(&table_id).await.unwrap().game_config.rng_seed,
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
                        "playerId": &pids[0],
                        "seq": seq,
                        "cards": ["♠3", "♥3", "♦3", "♣3"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let snap = snapshot_player(&app, &table_id, &pids[1]).await;
    assert!(
        snap["narration"]
            .as_str()
            .is_some_and(|s| s.contains("炸")),
        "expected bomb narration, got {:?}",
        snap["narration"]
    );
}
