//! Integration tests: HTTP API + seq / nextstate.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use clawguandan::api::app_with_store;
use clawguandan::store::TableStore;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use tower::ServiceExt;

async fn read_json(res: axum::response::Response) -> Value {
    let body = res.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn list_tables_empty_and_lobby() {
    let app = app_with_store(TableStore::new());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/tables")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v = read_json(res).await;
    assert_eq!(v["tables"], json!([]));

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/tables")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"lobby-x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let create = read_json(res).await;
    let table_id = create["tableId"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/tables")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list = read_json(res).await;
    let tables = list["tables"].as_array().unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0]["name"], "lobby-x");
    assert_eq!(tables[0]["state"]["tableId"], table_id);
    assert!(tables[0]["state"]["hand"].is_null());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/tables?detail=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list_d = read_json(res).await;
    assert!(list_d["tables"][0]["state"]["hand"].is_null());
}

#[tokio::test]
async fn list_tables_detail_hand_matches_observer_snapshot() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, _pids, _keys, _seq) = create_ready_table(app).await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/tables")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list = read_json(res).await;
    let row = &list["tables"][0];
    assert_eq!(row["state"]["tableId"], table_id);
    assert!(row["state"]["hand"].is_null());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/tables/{}/snapshot", table_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let snap = read_json(res).await;
    assert!(!snap["hand"].is_null());

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/tables?detail=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list_d = read_json(res).await;
    assert_eq!(list_d["tables"][0]["state"]["hand"], snap["hand"]);
}

async fn create_ready_table(
    app: axum::Router,
) -> (
    axum::Router,
    String,
    Vec<String>,
    HashMap<String, String>,
    u64,
) {
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
    let mut keys: HashMap<String, String> = HashMap::new();
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
        let pid = body["playerId"].as_str().unwrap().to_string();
        let pkey = body["playerKey"].as_str().unwrap().to_string();
        keys.insert(pid.clone(), pkey);
        pids.push(pid);
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
                        json!({
                            "playerId": pid,
                            "playerKey": keys.get(pid).unwrap(),
                            "ready": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = read_json(res).await;
        seq = body["newSeq"].as_u64().unwrap();
    }
    (app, table_id, pids, keys, seq)
}

#[tokio::test]
async fn create_join_ready_game_started_flow() {
    let app = app_with_store(TableStore::new());

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
    assert_eq!(res.status(), StatusCode::OK);
    let v = read_json(res).await;
    let table_id = v["tableId"].as_str().unwrap().to_string();
    assert_eq!(v["seq"], 0);
    assert_eq!(v["status"], "waiting");

    let mut pids = Vec::new();
    let mut keys: HashMap<String, String> = HashMap::new();
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
        assert_eq!(res.status(), StatusCode::OK, "join {}", i);
        let body = read_json(res).await;
        let pid = body["playerId"].as_str().unwrap().to_string();
        let pkey = body["playerKey"].as_str().unwrap().to_string();
        keys.insert(pid.clone(), pkey);
        pids.push(pid);
    }
    let last_pid = pids.last().cloned().unwrap();

    // Optional: first transition is seq=1 (join #1)
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq=0&timeoutMs=100",
                    table_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let t1 = read_json(res).await;
    assert_eq!(t1["seq"], 1);
    assert_eq!(t1["lag"], 3);

    // After four joins the table head is seq=4; ready does not require client seq.
    for pid in &pids {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/tables/{}/ready", table_id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "playerId": pid,
                            "playerKey": keys.get(pid).unwrap(),
                            "ready": true,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "ready {}", pid);
        let _body = read_json(res).await;
    }

    // Last ready emitted GAME_STARTED as transition 8 — catch up from seq 7
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq=7&timeoutMs=100",
                    table_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let t_last = read_json(res).await;
    assert_eq!(t_last["seq"], 8);
    assert_eq!(t_last["lag"], 0);
    assert_eq!(t_last["type"], "GAME_STARTED");
    assert_eq!(t_last["expect"]["kind"], "play");
    assert_eq!(
        t_last["expect"]["actorPlayerIds"].as_array(),
        Some(&vec![json!(pids[0].as_str())]),
        "first joiner sits E; first hand opens at E"
    );
    // Observer mode: `private` must not be present.
    assert!(t_last.get("private").is_none());
    assert_eq!(
        t_last["delta"]["event"]["trigger"]["actionType"].as_str(),
        Some("ready")
    );
    assert_eq!(
        t_last["delta"]["event"]["trigger"]["actorPlayerId"].as_str(),
        Some(last_pid.as_str())
    );
}

#[tokio::test]
async fn join_player_model_only_effective_for_bot() {
    let app = app_with_store(TableStore::new());

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

    let bot_join = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerName": "Bot-E",
                        "playerType": "bot",
                        "playerModel": "  gpt-4o  ",
                        "seat": "E",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bot_join.status(), StatusCode::OK);
    let bot_body = read_json(bot_join).await;
    assert_eq!(bot_body["playerType"], "bot");
    assert_eq!(bot_body["playerModel"], "gpt-4o");

    let human_join = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerName": "H-S",
                        "playerType": "human",
                        "playerModel": "ignored-model",
                        "seat": "S",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(human_join.status(), StatusCode::OK);
    let human_body = read_json(human_join).await;
    assert!(human_body["playerModel"].is_null());

    let unknown_join = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerName": "U-W",
                        "playerModel": "ignored-too",
                        "seat": "W",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown_join.status(), StatusCode::OK);
    let unknown_body = read_json(unknown_join).await;
    assert_eq!(unknown_body["playerType"], "unknown");
    assert!(unknown_body["playerModel"].is_null());

    let snap_res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/tables/{}/snapshot", table_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snap_res.status(), StatusCode::OK);
    let snap = read_json(snap_res).await;
    assert_eq!(snap["seats"]["E"]["playerModel"], "gpt-4o");
    assert!(snap["seats"]["S"]["playerModel"].is_null());
    assert!(snap["seats"]["W"]["playerModel"].is_null());
}

#[tokio::test]
async fn join_bot_with_blank_model_normalizes_to_none() {
    let app = app_with_store(TableStore::new());

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

    let join = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerName": "Bot-E",
                        "playerType": "bot",
                        "playerModel": "   ",
                        "seat": "E",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(join.status(), StatusCode::OK);
    let join_body = read_json(join).await;
    assert!(join_body["playerModel"].is_null());

    let snap_res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/tables/{}/snapshot", table_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(snap_res.status(), StatusCode::OK);
    let snap = read_json(snap_res).await;
    assert!(snap["seats"]["E"]["playerModel"].is_null());
}

#[tokio::test]
async fn ready_idempotent_does_not_advance_seq() {
    let app = app_with_store(TableStore::new());

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
    let table_id = v["tableId"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerName": "A", "seat": "auto"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let join = read_json(res).await;
    let pid = join["playerId"].as_str().unwrap();
    let pkey = join["playerKey"].as_str().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/ready", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": pid,
                        "playerKey": pkey,
                        "ready": true,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let r1 = read_json(res).await;
    let head_after_first_ready = r1["newSeq"].as_u64().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/ready", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": pid,
                        "playerKey": pkey,
                        "ready": true,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let r2 = read_json(res).await;
    assert_eq!(r2["newSeq"], head_after_first_ready);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/tables/{}/snapshot", table_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let snap = read_json(res).await;
    assert_eq!(snap["seq"], head_after_first_ready);
}

#[tokio::test]
async fn snapshot_private_visibility() {
    let app = app_with_store(TableStore::new());

    // Create table.
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
    let table_id = v["tableId"].as_str().unwrap();

    // Join one player.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerName": "A", "seat": "auto"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let join = read_json(res).await;
    let pid = join["playerId"].as_str().unwrap();
    let pkey = join["playerKey"].as_str().unwrap();
    let seat = join["seat"].as_str().unwrap();

    // Observer snapshot: no `private`.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/tables/{}/snapshot", table_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let snap_obs = read_json(res).await;
    assert!(snap_obs.get("private").is_none());

    // Player snapshot: `private` exists.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/snapshot?playerId={}&playerKey={}",
                    table_id, pid, pkey
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let snap_p = read_json(res).await;
    assert!(snap_p.get("private").is_some());
    assert_eq!(snap_p["private"]["seat"].as_str(), Some(seat));
    let teammate = snap_p["private"]["teammateSeat"].as_str().unwrap_or("");
    let expected_teammate = match seat {
        "E" => "W",
        "W" => "E",
        "S" => "N",
        "N" => "S",
        _ => "",
    };
    assert_eq!(teammate, expected_teammate, "seat={seat}");
}

#[tokio::test]
async fn nextstate_observer_has_prompt_and_no_private() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, keys, seq) = create_ready_table(app).await;
    assert!(seq >= 1);

    let observer_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=100",
                    table_id,
                    seq - 1
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(observer_res.status(), StatusCode::OK);
    let observer_body = read_json(observer_res).await;
    assert!(observer_body.get("private").is_none());
    assert!(
        observer_body["prompt"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );

    let pid = &pids[0];
    let pkey = keys.get(pid).unwrap();
    let player_res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=100&playerId={}&playerKey={}",
                    table_id,
                    seq - 1,
                    pid,
                    pkey
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(player_res.status(), StatusCode::OK);
    let player_body = read_json(player_res).await;
    assert!(player_body.get("private").is_some());
    assert!(
        player_body["prompt"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

#[tokio::test]
async fn nextstate_observer_head_timeout_returns_204_with_headers() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, _pids, _keys, seq) = create_ready_table(app).await;
    let exp_seq = seq.to_string();
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=1",
                    table_id, seq
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        res.headers()
            .get("x-table-seq")
            .and_then(|h| h.to_str().ok()),
        Some(exp_seq.as_str())
    );
    assert_eq!(
        res.headers()
            .get("x-table-lag")
            .and_then(|h| h.to_str().ok()),
        Some("0")
    );
}

#[tokio::test]
async fn join_returns_player_key() {
    let app = app_with_store(TableStore::new());
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
    let table_id = read_json(res).await["tableId"]
        .as_str()
        .unwrap()
        .to_string();

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerName":"A","seat":"auto"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = read_json(res).await;
    assert!(body["playerKey"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn snapshot_with_player_id_requires_player_key() {
    let app = app_with_store(TableStore::new());
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
    let table_id = read_json(res).await["tableId"]
        .as_str()
        .unwrap()
        .to_string();
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/join", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerName":"A","seat":"auto"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let pid = read_json(res).await["playerId"]
        .as_str()
        .unwrap()
        .to_string();

    let res = app
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
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn action_with_wrong_player_key_is_forbidden() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, _keys, seq) = create_ready_table(app).await;
    let pid = &pids[0];
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": pid,
                        "playerKey": "wrong-key",
                        "seq": seq
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn action_endpoints_advance_seq_and_emit_transition() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, keys, mut seq) = create_ready_table(app).await;
    let pid = &pids[0];

    // First hand skips tribute; E leads — pass is legal when following (here: wrong test if leading with cards).
    // Use pass only when engine allows: actually E leads with cards so pass fails. Use a minimal legal play:
    // take first two identical ranks from snapshot private hand as pair, or pass is illegal for leader with cards.
    // So we only verify a successful engine-backed action: wrong-phase return_card already tested elsewhere.
    // Here: `pass` from a non-actor fails with 422 — instead call `pass` when it's wrong turn from another seat.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": &pids[1],
                        "playerKey": keys.get(&pids[1]).unwrap(),
                        "seq": seq
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": pid,
                        "playerKey": keys.get(pid).unwrap(),
                        "seq": seq
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "leader with cards cannot pass"
    );

    // Legal play: use suggest — `handCards[0]` is flaky (random table seed may put ♥2 / joker first).
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/suggest?playerId={}&playerKey={}&seq={}",
                    table_id,
                    pid,
                    keys.get(pid).unwrap(),
                    seq
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "suggest for acting player");
    let sug = read_json(res).await;
    assert_eq!(sug["actionType"], "play");
    let payload = &sug["payload"];
    let mut play_body = json!({
        "playerId": pid,
        "playerKey": keys.get(pid).unwrap(),
        "seq": seq,
        "cards": payload["cards"].clone(),
    });
    if let Some(dm) = payload.get("declaredWildMapping") {
        play_body["declaredWildMapping"] = dm.clone();
    }
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/play", table_id))
                .header("content-type", "application/json")
                .body(Body::from(play_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = read_json(res).await;
    seq = body["newSeq"].as_u64().unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=10",
                    table_id,
                    seq - 1
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let tr = read_json(res).await;
    assert_eq!(tr["type"], "ACTION_APPLIED");
    assert_eq!(tr["lag"], 0);
    assert_eq!(tr["delta"]["event"]["trigger"]["actionType"], "play");
}

#[tokio::test]
async fn failed_action_does_not_advance_seq() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, keys, seq) = create_ready_table(app).await;
    let pid = &pids[0];

    // return_card in Dealing should fail with 422.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/return_card", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": pid,
                        "playerKey": keys.get(pid).unwrap(),
                        "seq": seq,
                        "card": "♠3"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let err = read_json(res).await;
    assert_eq!(err["error"]["currentSeq"], seq);

    // sinceSeq==currentSeq should timeout (204), proving no new transition.
    let res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/v1/tables/{}/nextstate?sinceSeq={}&timeoutMs=1",
                    table_id, seq
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let exp_seq = seq.to_string();
    assert_eq!(
        res.headers()
            .get("x-table-seq")
            .and_then(|h| h.to_str().ok()),
        Some(exp_seq.as_str())
    );
    assert_eq!(
        res.headers()
            .get("x-table-lag")
            .and_then(|h| h.to_str().ok()),
        Some("0")
    );
}

#[tokio::test]
async fn action_stale_seq_returns_409() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, keys, seq) = create_ready_table(app).await;
    let pid = &pids[0];

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": pid,
                        "playerKey": keys.get(pid).unwrap(),
                        "seq": seq - 1
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let v = read_json(res).await;
    assert_eq!(v["error"]["code"], "STALE_SEQ");
    assert_eq!(v["error"]["currentSeq"], seq);
}

#[tokio::test]
async fn action_non_seated_player_returns_403() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, _pids, _keys, seq) = create_ready_table(app).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": "p_not_seated",
                        "playerKey": "wrong-key",
                        "seq": seq
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
