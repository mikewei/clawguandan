//! HTTP-only helpers: [`GET /suggest`](crate::api::router) then [`POST actions/*`](crate::api::router).

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde::Deserialize;
use std::collections::HashMap;
use tower::ServiceExt;

use crate::error::AppError;
use crate::game::engine::PlayerAction;
use crate::game::types::GamePhase;
use crate::store::TableStore;
use crate::strategy::current_actor_seat;

#[derive(Debug)]
pub enum HttpSimError {
    Store(AppError),
    Movegen(String),
    NoActor,
    MissingPlayerKey(String),
    MaxPlies,
    Router(String),
}

impl std::fmt::Display for HttpSimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpSimError::Store(e) => write!(f, "{e}"),
            HttpSimError::Movegen(s) => write!(f, "{s}"),
            HttpSimError::NoActor => write!(f, "no player_id for actor seat"),
            HttpSimError::MissingPlayerKey(pid) => write!(f, "missing player_key for player_id {pid}"),
            HttpSimError::MaxPlies => write!(f, "max_plies exceeded"),
            HttpSimError::Router(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for HttpSimError {}

impl From<AppError> for HttpSimError {
    fn from(e: AppError) -> Self {
        HttpSimError::Store(e)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuggestHttpBody {
    #[allow(dead_code)]
    seq: u64,
    #[allow(dead_code)]
    actor_player_id: String,
    action_type: String,
    payload: serde_json::Value,
}

async fn get_suggest_router(
    app: Router,
    table_id: &str,
    seq: u64,
    player_id: &str,
    player_key: &str,
) -> Result<SuggestHttpBody, HttpSimError> {
    let uri = format!(
        "/api/v1/tables/{}/suggest?seq={}&playerId={}&playerKey={}",
        table_id, seq, player_id, player_key
    );
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .map_err(|e| HttpSimError::Router(e.to_string()))?;
    let res = app
        .oneshot(req)
        .await
        .map_err(|e| HttpSimError::Router(e.to_string()))?;
    let status = res.status();
    let body = res.into_body();
    let bytes = body
        .collect()
        .await
        .map_err(|e| HttpSimError::Router(e.to_string()))?
        .to_bytes();
    if !status.is_success() {
        return Err(HttpSimError::Router(format!(
            "GET suggest HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
        )));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| HttpSimError::Router(e.to_string()))?;
    serde_json::from_value(v).map_err(|e| HttpSimError::Router(e.to_string()))
}

async fn post_action_router(
    app: Router,
    table_id: &str,
    action: &PlayerAction,
    player_id: &str,
    player_key: &str,
    seq: u64,
) -> Result<u64, HttpSimError> {
    let (suffix, body) = action.to_http_action_request(player_id, seq);
    let mut body_obj = body;
    body_obj["playerKey"] = serde_json::json!(player_key);
    let uri = format!("/api/v1/tables/{}/actions/{}", table_id, suffix);
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&body_obj).map_err(|e| HttpSimError::Router(e.to_string()))?,
        ))
        .map_err(|e| HttpSimError::Router(e.to_string()))?;
    let res = app
        .oneshot(req)
        .await
        .map_err(|e| HttpSimError::Router(e.to_string()))?;
    let status = res.status();
    let body = res.into_body();
    let bytes = body
        .collect()
        .await
        .map_err(|e| HttpSimError::Router(e.to_string()))?
        .to_bytes();
    if !status.is_success() {
        return Err(HttpSimError::Router(format!(
            "HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
        )));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| HttpSimError::Router(e.to_string()))?;
    v.get("newSeq")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| HttpSimError::Router("missing newSeq in response".into()))
}

/// Run one hand until [`GamePhase::Scoring`] using GET suggest + POST actions on `app`.
pub async fn run_hand_until_scoring_via_router(
    app: Router,
    store: &TableStore,
    table_id: &str,
    player_keys: &HashMap<String, String>,
    max_plies: usize,
) -> Result<(), HttpSimError> {
    let mut plies = 0usize;
    loop {
        if plies >= max_plies {
            return Err(HttpSimError::MaxPlies);
        }
        let snap = store.get_snapshot(table_id).await?;
        let seq = snap.seq;
        let Some(game) = snap.game.clone() else {
            return Ok(());
        };
        if game.phase == GamePhase::Scoring {
            return Ok(());
        }
        let Some(actor) = current_actor_seat(&game) else {
            return Err(HttpSimError::NoActor);
        };
        let Some(pid) = snap.player_id_for_seat(actor) else {
            return Err(HttpSimError::NoActor);
        };
        let pkey = player_keys
            .get(&pid)
            .ok_or_else(|| HttpSimError::MissingPlayerKey(pid.clone()))?;
        let sug = get_suggest_router(app.clone(), table_id, seq, &pid, pkey).await?;
        let action = PlayerAction::try_from_action_type_payload(&sug.action_type, &sug.payload)
            .map_err(HttpSimError::Movegen)?;
        post_action_router(app.clone(), table_id, &action, &pid, pkey, seq).await?;
        plies += 1;
    }
}
