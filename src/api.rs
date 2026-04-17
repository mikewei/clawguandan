use crate::domain::{Phase, PlayerType, PrivateView, TableState, TableStatus};
use crate::error::AppError;
use crate::game::rules::scoring::Level;
use crate::game::types::GamePhase;
use crate::store::{SeatOrAuto, TableStore};
use crate::strategy::{current_actor_seat, suggest_next_action};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone)]
pub struct AppState {
    pub store: TableStore,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ping", get(ping))
        .route("/api/v1/tables", get(list_tables).post(create_table))
        .route("/api/v1/tables/:table_id/join", post(join_table))
        .route("/api/v1/tables/:table_id/ready", post(ready))
        .route(
            "/api/v1/tables/:table_id/actions/tribute",
            post(action_tribute),
        )
        .route(
            "/api/v1/tables/:table_id/actions/return_card",
            post(action_return_card),
        )
        .route("/api/v1/tables/:table_id/actions/play", post(action_play))
        .route("/api/v1/tables/:table_id/actions/pass", post(action_pass))
        .route(
            "/api/v1/tables/:table_id/nextstate",
            get(nextstate).post(nextstate_post),
        )
        .route("/api/v1/tables/:table_id/snapshot", get(snapshot))
        .route("/api/v1/tables/:table_id/suggest", get(suggest))
        .fallback(get(crate::web_assets::serve_embedded))
        .with_state(state)
}

async fn ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "pong": "clawguandan",
        "ver": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
    }))
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateTableBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rank: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTableResponse {
    pub table_id: String,
    pub seq: u64,
    pub status: TableStatus,
}

async fn create_table(
    State(state): State<AppState>,
    Json(body): Json<CreateTableBody>,
) -> Result<Json<CreateTableResponse>, AppError> {
    let start_level = parse_create_rank(body.rank.as_deref())?;
    let t = state.store.create_table(body.name, start_level).await;
    Ok(Json(CreateTableResponse {
        table_id: t.table_id.clone(),
        seq: t.seq,
        status: t.status.clone(),
    }))
}

fn parse_create_rank(rank: Option<&str>) -> Result<Level, AppError> {
    let Some(rank) = rank else {
        return Ok(Level::Two);
    };
    match rank.trim().to_ascii_uppercase().as_str() {
        "2" => Ok(Level::Two),
        "3" => Ok(Level::Three),
        "4" => Ok(Level::Four),
        "5" => Ok(Level::Five),
        "6" => Ok(Level::Six),
        "7" => Ok(Level::Seven),
        "8" => Ok(Level::Eight),
        "9" => Ok(Level::Nine),
        "10" => Ok(Level::Ten),
        "J" => Ok(Level::J),
        "Q" => Ok(Level::Q),
        "K" => Ok(Level::K),
        "A" => Ok(Level::A),
        _ => Err(AppError::BadRequest(
            "invalid rank; allowed values: 2-10, J, Q, K, A".into(),
        )),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesQuery {
    #[serde(default)]
    pub detail: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub state: TableState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTablesResponse {
    pub tables: Vec<ListTablesEntry>,
}

async fn list_tables(
    State(state): State<AppState>,
    Query(q): Query<ListTablesQuery>,
) -> Result<Json<ListTablesResponse>, AppError> {
    let runtimes = state.store.list_table_runtimes().await;
    let tables = runtimes
        .into_iter()
        .map(|runtime| {
            let name = runtime.table_name.clone();
            let mut state = runtime.to_table_state();
            if !q.detail {
                state.hand = None;
            }
            ListTablesEntry { name, state }
        })
        .collect();
    Ok(Json(ListTablesResponse { tables }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinBody {
    #[serde(default)]
    pub player_type: Option<PlayerType>,
    #[serde(default)]
    pub player_model: Option<String>,
    pub player_name: String,
    #[serde(default = "default_seat_auto")]
    pub seat: String,
}

fn default_seat_auto() -> String {
    "auto".into()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinResponse {
    pub player_id: String,
    pub seat: String,
    pub player_type: PlayerType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_model: Option<String>,
    pub new_seq: u64,
}

async fn join_table(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<JoinBody>,
) -> Result<Json<JoinResponse>, AppError> {
    let seat = SeatOrAuto::parse(&body.seat)?;
    let (pid, seat, pt, player_model) = state
        .store
        .join(
            &table_id,
            body.player_name,
            body.player_type,
            body.player_model,
            seat,
        )
        .await?;
    let snap = state.store.get_snapshot(&table_id).await?;
    Ok(Json(JoinResponse {
        player_id: pid,
        seat: seat.as_str().into(),
        player_type: pt,
        player_model,
        new_seq: snap.seq,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyBody {
    pub player_id: String,
    pub ready: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyResponse {
    pub new_seq: u64,
    pub ready: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TributeBody {
    pub player_id: String,
    pub seq: u64,
    pub card: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnCardBody {
    pub player_id: String,
    pub seq: u64,
    pub card: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayBody {
    pub player_id: String,
    pub seq: u64,
    pub cards: Vec<String>,
    pub declared_wild_mapping: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassBody {
    pub player_id: String,
    pub seq: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResponse {
    pub new_seq: u64,
}

async fn ready(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<ReadyBody>,
) -> Result<Json<ReadyResponse>, AppError> {
    let new_seq = state
        .store
        .set_ready(&table_id, &body.player_id, body.ready)
        .await?;
    Ok(Json(ReadyResponse {
        new_seq,
        ready: body.ready,
    }))
}

async fn action_tribute(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<TributeBody>,
) -> Result<Json<ActionResponse>, AppError> {
    let new_seq = state
        .store
        .apply_action(
            &table_id,
            &body.player_id,
            body.seq,
            "tribute",
            serde_json::json!({ "card": body.card }),
        )
        .await?;
    Ok(Json(ActionResponse { new_seq }))
}

async fn action_return_card(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<ReturnCardBody>,
) -> Result<Json<ActionResponse>, AppError> {
    let new_seq = state
        .store
        .apply_action(
            &table_id,
            &body.player_id,
            body.seq,
            "return_card",
            serde_json::json!({ "card": body.card }),
        )
        .await?;
    Ok(Json(ActionResponse { new_seq }))
}

async fn action_play(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<PlayBody>,
) -> Result<Json<ActionResponse>, AppError> {
    let new_seq = state
        .store
        .apply_action(
            &table_id,
            &body.player_id,
            body.seq,
            "play",
            serde_json::json!({
                "cards": body.cards,
                "declaredWildMapping": body.declared_wild_mapping
            }),
        )
        .await?;
    Ok(Json(ActionResponse { new_seq }))
}

async fn action_pass(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<PassBody>,
) -> Result<Json<ActionResponse>, AppError> {
    let new_seq = state
        .store
        .apply_action(
            &table_id,
            &body.player_id,
            body.seq,
            "pass",
            serde_json::json!({}),
        )
        .await?;
    Ok(Json(ActionResponse { new_seq }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestQuery {
    pub seq: u64,
    pub player_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestResponse {
    pub seq: u64,
    pub actor_player_id: String,
    pub action_type: String,
    pub payload: serde_json::Value,
    pub phase: Phase,
}

async fn suggest(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Query(q): Query<SuggestQuery>,
) -> Result<Json<SuggestResponse>, AppError> {
    state
        .store
        .touch_player_activity(&table_id, &q.player_id)
        .await?;
    let snap = state.store.get_snapshot(&table_id).await?;
    if q.seq != snap.seq {
        return Err(AppError::Conflict {
            message: format!("stale seq: expected {}, got {}", snap.seq, q.seq),
            code: "STALE_SEQ",
            current_seq: Some(snap.seq),
        });
    }
    if !matches!(snap.status, TableStatus::InGame) {
        return Err(AppError::Conflict {
            message: "suggest is only allowed when table is in_game".into(),
            code: "INVALID_TABLE_STATUS",
            current_seq: Some(snap.seq),
        });
    }
    let game = snap.game.as_ref().ok_or_else(|| AppError::Conflict {
        message: "game state not initialized".into(),
        code: "INVALID_TABLE_STATUS",
        current_seq: Some(snap.seq),
    })?;
    if matches!(game.phase, GamePhase::Scoring) {
        return Err(AppError::BadRequest(
            "hand finished (scoring); wait for next hand to start".into(),
        ));
    }
    let actor = current_actor_seat(game)
        .ok_or_else(|| AppError::BadRequest("no actor for current phase".into()))?;
    let expected_pid = snap
        .player_id_for_seat(actor)
        .ok_or_else(|| AppError::BadRequest("missing player_id for actor seat".into()))?;
    if q.player_id != expected_pid {
        return Err(AppError::Forbidden(
            "player_id is not the current actor for this table".into(),
        ));
    }
    let action = suggest_next_action(game, actor).map_err(AppError::BadRequest)?;
    let (action_type, payload) = action.to_store_payload();
    Ok(Json(SuggestResponse {
        seq: snap.seq,
        actor_player_id: expected_pid,
        action_type: action_type.to_string(),
        payload,
        phase: snap.phase.clone(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextStateQuery {
    pub since_seq: u64,
    pub player_id: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextStatePostBody {
    pub since_seq: u64,
    pub player_id: Option<String>,
    pub timeout_ms: Option<u64>,
}

async fn nextstate(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Query(q): Query<NextStateQuery>,
) -> Result<impl IntoResponse, AppError> {
    nextstate_inner(state, table_id, q).await
}

async fn nextstate_post(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Json(body): Json<NextStatePostBody>,
) -> Result<impl IntoResponse, AppError> {
    nextstate_inner(
        state,
        table_id,
        NextStateQuery {
            since_seq: body.since_seq,
            player_id: body.player_id,
            timeout_ms: body.timeout_ms,
        },
    )
    .await
}

async fn nextstate_inner(
    state: AppState,
    table_id: String,
    q: NextStateQuery,
) -> Result<impl IntoResponse, AppError> {
    let timeout = q.timeout_ms.map(std::time::Duration::from_millis);
    let body = state
        .store
        .next_state_with_prompt(&table_id, q.since_seq, q.player_id.as_deref(), timeout)
        .await?;
    match body {
        Some(b) => Ok((StatusCode::OK, Json(b)).into_response()),
        None => {
            let snap = state.store.get_snapshot(&table_id).await?;
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::NO_CONTENT;
            res.headers_mut().insert(
                HeaderName::from_static("x-table-seq"),
                HeaderValue::from_str(&snap.seq.to_string())
                    .map_err(|_| AppError::BadRequest("invalid X-Table-Seq header value".into()))?,
            );
            res.headers_mut().insert(
                HeaderName::from_static("x-table-lag"),
                HeaderValue::from_static("0"),
            );
            Ok(res)
        }
    }
}

pub fn app() -> Router {
    router(AppState {
        store: TableStore::new(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    pub at_seq: Option<u64>,
    pub player_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotResponse {
    #[serde(flatten)]
    pub state: TableState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private: Option<PrivateView>,
}

async fn snapshot(
    State(state): State<AppState>,
    Path(table_id): Path<String>,
    Query(q): Query<SnapshotQuery>,
) -> Result<Json<SnapshotResponse>, AppError> {
    if let Some(pid) = q.player_id.as_deref() {
        state.store.touch_player_activity(&table_id, pid).await?;
    }
    let snap = state.store.get_snapshot(&table_id).await?;
    if let Some(at_seq) = q.at_seq
        && at_seq != snap.seq
    {
        return Err(AppError::BadRequest(format!(
            "snapshot atSeq {} is not supported in MVP; currentSeq {}",
            at_seq, snap.seq
        )));
    }

    let table_state = snap.to_table_state();
    let private = q
        .player_id
        .and_then(|pid| snap.private_view_for_player(&pid));

    Ok(Json(SnapshotResponse {
        state: table_state,
        private,
    }))
}

/// Test helper: router with injected store.
pub fn app_with_store(store: TableStore) -> Router {
    router(AppState { store })
}
