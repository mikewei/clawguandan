//! 错误码与响应体字段：`409` stale seq、`403` 非座位、`422` 引擎拒绝且不推进 seq。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use clawguandan::api::app_with_store;
use clawguandan::store::TableStore;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

async fn read_json(res: axum::response::Response) -> serde_json::Value {
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
async fn unprocessable_includes_code_and_current_seq() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, seq) = create_ready_table(app).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[1], "seq": seq}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = read_json(res).await;
    assert_eq!(v["error"]["code"], "WRONG_TURN");
    assert_eq!(v["error"]["currentSeq"], seq);
}

#[tokio::test]
async fn illegal_play_includes_illegal_action_or_similar() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, seq) = create_ready_table(app).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/play", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "playerId": &pids[0],
                        "seq": seq,
                        "cards": ["♠A", "♥K"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = read_json(res).await;
    assert_eq!(v["error"]["code"], "ILLEGAL_ACTION");
    assert_eq!(v["error"]["currentSeq"], seq);
}

#[tokio::test]
async fn conflict_stale_seq_shape() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, pids, seq) = create_ready_table(app).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": &pids[0], "seq": seq - 1}).to_string(),
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
async fn forbidden_non_seated_shape() {
    let app = app_with_store(TableStore::new());
    let (app, table_id, _pids, seq) = create_ready_table(app).await;

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/tables/{}/actions/pass", table_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"playerId": "p_not_here", "seq": seq}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
