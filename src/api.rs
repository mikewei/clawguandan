use crate::domain::{Phase, PlayerType, PrivateView, TableState, TableStatus};
use crate::error::AppError;
use crate::game::rules::scoring::Level;
use crate::game::types::GamePhase;
use crate::lan_addrs::lan_http_base_urls;
use crate::store::{SeatOrAuto, TableStore};
use crate::strategy::{current_actor_seat, suggest_next_action};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde::Serialize;
use std::net::IpAddr;

#[derive(Clone)]
pub struct AppState {
    pub store: TableStore,
    /// TCP listen port (for `/ping` `lanWebUrls`, LAN first then WAN).
    pub listen_port: u16,
    /// Address passed to `bind(2)` (e.g. `0.0.0.0` expands to interface LAN IPv4s).
    pub bind_ip: IpAddr,
}

impl AppState {
    /// Router tests: loopback bind ⇒ empty `lanWebUrls`.
    pub fn for_tests(store: TableStore) -> Self {
        Self {
            store,
            listen_port: 22_222,
            bind_ip: std::net::Ipv4Addr::LOCALHOST.into(),
        }
    }
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
        .route("/api/v1/rules", get(rules))
        .fallback(get(crate::web_assets::serve_embedded))
        .with_state(state)
}

async fn ping(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Backward-compatible field name. Values may include WAN URLs after LAN URLs.
    let lan_web_urls = lan_http_base_urls(state.listen_port, state.bind_ip);
    Json(serde_json::json!({
        "pong": "clawguandan",
        "ver": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "lanWebUrls": lan_web_urls,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesQuery {
    #[serde(default)]
    pub lang: Option<String>,
}

async fn rules(Query(q): Query<RulesQuery>) -> Result<impl IntoResponse, AppError> {
    let md = crate::web_assets::rules_markdown(q.lang.as_deref()).map_err(AppError::BadRequest)?;
    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/markdown; charset=utf-8"),
        )],
        md,
    ))
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
    Level::from_api_str(rank).ok_or_else(|| {
        AppError::BadRequest("invalid rank; allowed values: 2-10, J, Q, K, A".into())
    })
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
    pub player_key: String,
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
    let (pid, pkey, seat, pt, player_model) = state
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
        player_key: pkey,
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
    pub player_key: String,
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
    pub player_key: String,
    pub seq: u64,
    pub card: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnCardBody {
    pub player_id: String,
    pub player_key: String,
    pub seq: u64,
    pub card: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayBody {
    pub player_id: String,
    pub player_key: String,
    pub seq: u64,
    pub cards: Vec<String>,
    pub declared_wild_mapping: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassBody {
    pub player_id: String,
    pub player_key: String,
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
        .set_ready(&table_id, &body.player_id, &body.player_key, body.ready)
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
            &body.player_key,
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
            &body.player_key,
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
            &body.player_key,
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
            &body.player_key,
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
    pub player_key: String,
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
        .verify_player_identity(&table_id, &q.player_id, &q.player_key)
        .await?;
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
    pub player_key: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextStatePostBody {
    pub since_seq: u64,
    pub player_id: Option<String>,
    pub player_key: Option<String>,
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
            player_key: body.player_key,
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
        .next_state_with_prompt(
            &table_id,
            q.since_seq,
            q.player_id.as_deref(),
            q.player_key.as_deref(),
            timeout,
        )
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
    router(AppState::for_tests(TableStore::new()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    pub at_seq: Option<u64>,
    pub player_id: Option<String>,
    pub player_key: Option<String>,
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
    let snap = state.store.get_snapshot(&table_id).await?;
    if let Some(at_seq) = q.at_seq
        && at_seq != snap.seq
    {
        return Err(AppError::BadRequest(format!(
            "snapshot atSeq {} is not supported in MVP; currentSeq {}",
            at_seq, snap.seq
        )));
    }
    if let Some(ref pid) = q.player_id {
        let pkey = q
            .player_key
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("playerKey is required with playerId".into()))?;
        state
            .store
            .verify_player_identity(&table_id, pid.as_str(), pkey)
            .await?;
        state
            .store
            .touch_player_activity(&table_id, pid.as_str())
            .await?;
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
    router(AppState::for_tests(store))
}
