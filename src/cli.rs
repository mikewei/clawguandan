use clap::{Parser, Subcommand};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use url::Url;

use clawguandan::domain::{
    NextStateBody, PrivateView, TableState, TableStatus, apply_transition_delta_to_table_state,
};
use clawguandan::game::engine::PlayerAction;
use clawguandan::simulation::run_cli_command;
use clawguandan::web_assets::rules_markdown;

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clawguandan")
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Default local bind for `server start` / auto-config probe.
pub(crate) const LOCAL_SERVER_PROBE_ADDR: &str = "127.0.0.1:22222";

#[derive(Serialize, Deserialize, Default)]
struct CliConfig {
    server_url: Option<String>,
}

/// Per-session CLI state under `std::env::temp_dir()/clawguandan/<session_key>/session.json`:
/// - Players: `<session_key> = <hostPortKey>.<table_id>.<player_id>` (`hostPortKey` derived from `server_url`)
/// - Observers: `<session_key> = <hostPortKey>.<table_id>.observer`
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct PlayerSession {
    /// Schema version for forward-compatible reads.
    version: u32,
    last_applied_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_state: Option<TableState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    private_view: Option<PrivateView>,
}

impl Default for PlayerSession {
    fn default() -> Self {
        Self {
            version: 1,
            last_applied_seq: 0,
            table_state: None,
            private_view: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct AuthSession {
    version: u32,
    player_id: String,
    player_key: String,
}

impl Default for AuthSession {
    fn default() -> Self {
        Self {
            version: 1,
            player_id: String::new(),
            player_key: String::new(),
        }
    }
}

/// API shape of `GET .../snapshot` for deserialization.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotApiBody {
    #[serde(flatten)]
    state: TableState,
    private: Option<PrivateView>,
}

fn session_state_root() -> PathBuf {
    std::env::temp_dir().join("clawguandan")
}

/// Stable, path-safe prefix from active server URL (e.g. `http://127.0.0.1:22222` → `127.0.0.1_22222`).
fn session_host_port_key_from_base(base: &str) -> Result<String, String> {
    let normalized = normalize_base(base);
    let u = Url::parse(&normalized).map_err(|e| format!("invalid server URL: {e}"))?;
    let host = u
        .host_str()
        .ok_or_else(|| "server URL must include a host".to_string())?;
    let port = u
        .port_or_known_default()
        .ok_or_else(|| "server URL: could not determine port".to_string())?;
    let host_safe = host.replace(':', "_");
    if host_safe.is_empty() {
        return Err("server URL: empty host".into());
    }
    let key = format!("{host_safe}_{port}");
    validate_session_id_component(&key, "server host:port")?;
    Ok(key)
}

fn validate_session_id_component(s: &str, name: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err(format!("invalid {name}: empty"));
    }
    if s.contains('/') || s.contains('\\') || s.contains('\0') {
        return Err(format!(
            "invalid {name}: must not contain path separators or NUL"
        ));
    }
    Ok(())
}

fn session_json_path(session_key: &str) -> PathBuf {
    session_state_root().join(session_key).join("session.json")
}

fn auth_json_path(session_key: &str) -> PathBuf {
    session_state_root().join(session_key).join("auth.json")
}

/// Write `session.json` via temp file + rename (best-effort atomic replace).
fn write_session_state_atomic(path: &Path, json: &str) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "session path has no parent".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let fname = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "session path: missing file name".to_string())?;
    let tmp = dir.join(format!("{fname}.tmp"));
    fs::write(&tmp, json.as_bytes()).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(path);
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// If `session.json` is missing, try `last_applied_seq` from legacy `config.json`, migrate, strip key.
fn try_migrate_legacy_last_applied_seq(session_key: &str) -> Result<Option<u64>, String> {
    if session_key.contains('/') || session_key.contains('\\') || session_key.contains('\0') {
        return Ok(None);
    }
    let p = config_path();
    if !p.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let mut v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let seq = v
        .get("last_applied_seq")
        .and_then(|x| x.as_object())
        .and_then(|m| m.get(session_key))
        .and_then(|x| x.as_u64());
    let Some(seq) = seq else {
        return Ok(None);
    };

    let dest = session_json_path(session_key);
    let state = PlayerSession {
        last_applied_seq: seq,
        ..PlayerSession::default()
    };
    let s = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    write_session_state_atomic(&dest, &s)?;

    if let Some(obj) = v.as_object_mut() {
        if let Some(serde_json::Value::Object(m)) = obj.get_mut("last_applied_seq") {
            m.remove(session_key);
            if m.is_empty() {
                obj.remove("last_applied_seq");
            }
        }
    }
    fs::write(
        &p,
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(Some(seq))
}

fn read_player_session(
    base: &str,
    table_id: &str,
    player_id: &str,
) -> Result<Option<PlayerSession>, String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    validate_session_id_component(player_id, "player_id")?;
    let session_key = seq_key(&sid, table_id, player_id);
    let path = session_json_path(&session_key);
    if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let state: PlayerSession = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        return Ok(Some(state));
    }
    if let Some(seq) = try_migrate_legacy_last_applied_seq(&session_key)? {
        return Ok(Some(PlayerSession {
            last_applied_seq: seq,
            ..PlayerSession::default()
        }));
    }
    Ok(None)
}

fn write_player_session(
    base: &str,
    table_id: &str,
    player_id: &str,
    session: &PlayerSession,
) -> Result<(), String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    validate_session_id_component(player_id, "player_id")?;
    let session_key = seq_key(&sid, table_id, player_id);
    let path = session_json_path(&session_key);
    let s = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    write_session_state_atomic(&path, &s)
}

fn read_session_last_applied_seq(
    base: &str,
    table_id: &str,
    player_id: &str,
) -> Result<Option<u64>, String> {
    Ok(read_player_session(base, table_id, player_id)?.map(|s| s.last_applied_seq))
}

fn read_auth_session(base: &str, table_id: &str, player_id: &str) -> Result<Option<AuthSession>, String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    validate_session_id_component(player_id, "player_id")?;
    let session_key = seq_key(&sid, table_id, player_id);
    let path = auth_json_path(&session_key);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let auth: AuthSession = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(Some(auth))
}

fn write_auth_session(base: &str, table_id: &str, auth: &AuthSession) -> Result<(), String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    validate_session_id_component(&auth.player_id, "player_id")?;
    let session_key = seq_key(&sid, table_id, &auth.player_id);
    let path = auth_json_path(&session_key);
    let s = serde_json::to_string_pretty(auth).map_err(|e| e.to_string())?;
    write_session_state_atomic(&path, &s)
}

fn resolve_player_key(
    base: &str,
    table_id: &str,
    player_id: &str,
    explicit_player_key: Option<&str>,
) -> Result<String, String> {
    if let Some(k) = explicit_player_key {
        let trimmed = k.trim();
        if trimmed.is_empty() {
            return Err("playerKey cannot be empty".into());
        }
        return Ok(trimmed.to_string());
    }
    let auth = read_auth_session(base, table_id, player_id)?.ok_or_else(|| {
        "missing playerKey for this player; run `table join` again or pass `--player-key`".to_string()
    })?;
    if auth.player_id != player_id {
        return Err(format!(
            "auth playerId mismatch: expected {}, got {}",
            player_id, auth.player_id
        ));
    }
    if auth.player_key.trim().is_empty() {
        return Err("stored playerKey is empty; re-join or pass `--player-key`".into());
    }
    Ok(auth.player_key)
}

impl CliConfig {
    fn load() -> Self {
        let p = config_path();
        if !p.exists() {
            return Self::default();
        }
        let s = fs::read_to_string(&p).unwrap_or_default();
        serde_json::from_str(&s).unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(config_path(), s).map_err(|e| e.to_string())
    }
}

fn seq_key(host_port_key: &str, table_id: &str, player_id: &str) -> String {
    format!("{}.{}.{}", host_port_key, table_id, player_id)
}

/// Observer session uses the same flat layout as players: `seq_key(hostPortKey, table_id, "observer")`.
fn observer_session_json_path(base: &str, table_id: &str) -> Result<PathBuf, String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    Ok(session_json_path(&seq_key(&sid, table_id, "observer")))
}

fn read_observer_session(base: &str, table_id: &str) -> Result<Option<PlayerSession>, String> {
    let path = observer_session_json_path(base, table_id)?;
    if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let state: PlayerSession = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        return Ok(Some(state));
    }
    Ok(None)
}

fn write_observer_session(
    base: &str,
    table_id: &str,
    session: &PlayerSession,
) -> Result<(), String> {
    let path = observer_session_json_path(base, table_id)?;
    let s = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    write_session_state_atomic(&path, &s)
}

fn read_session_last_applied_seq_observer(
    base: &str,
    table_id: &str,
) -> Result<Option<u64>, String> {
    Ok(read_observer_session(base, table_id)?.map(|s| s.last_applied_seq))
}

fn base_url(cfg: &CliConfig) -> Result<String, String> {
    cfg.server_url
        .clone()
        .ok_or_else(|| "no active server: run `clawguandan server use <hostOrIp[:port]>`".into())
}

/// GET `{base}/ping`; returns JSON when `pong` is `clawguandan`.
fn fetch_ping_json_blocking(base: &str) -> Result<serde_json::Value, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/ping", normalize_base(base).trim_end_matches('/'));
    let r = client.get(&url).send().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    if v.get("pong").and_then(|x| x.as_str()) != Some("clawguandan") {
        return Err("not a clawguandan server (missing or wrong pong)".into());
    }
    Ok(v)
}

/// GET `{base}/ping`; returns API `ver` when `pong` is `clawguandan`.
fn ping_clawguandan_info(base: &str) -> Result<String, String> {
    let v = fetch_ping_json_blocking(base)?;
    Ok(v.get("ver")
        .and_then(|x| x.as_str())
        .unwrap_or("?")
        .to_string())
}

/// GET `{base}/ping` and verify `pong == "clawguandan"`.
fn probe_clawguandan_server(base: &str) -> Result<(), String> {
    ping_clawguandan_info(base)
        .map(|_| ())
        .map_err(|e| format!("probe failed: {e}"))
}

/// If `server_url` is unset, probe [`LOCAL_SERVER_PROBE_ADDR`] and persist config when valid.
fn try_autoconfigure_local_server() -> Result<(), String> {
    let mut cfg = CliConfig::load();
    if cfg.server_url.is_some() {
        return Ok(());
    }
    let local = normalize_base(LOCAL_SERVER_PROBE_ADDR);
    if probe_clawguandan_server(&local).is_ok() {
        cfg.server_url = Some(local);
        cfg.save()?;
    }
    Ok(())
}

fn load_active_server_base() -> Result<String, String> {
    try_autoconfigure_local_server()?;
    let cfg = CliConfig::load();
    base_url(&cfg)
}

fn normalize_base(url: &str) -> String {
    let u = url.trim();
    if u.starts_with("http://") || u.starts_with("https://") {
        u.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", u.trim_end_matches('/'))
    }
}

#[derive(Parser)]
#[command(
    name = "clawguandan",
    version,
    about = "Guan Dan — Server + API client"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Top,
}

#[derive(Subcommand)]
pub(crate) enum Top {
    /// Manage default server endpoint and run a server
    #[command(subcommand_required = false)]
    Server {
        #[command(subcommand)]
        cmd: Option<ServerCmd>,
    },
    /// Table commands
    Table {
        #[command(subcommand)]
        cmd: TableCmd,
    },
    /// Play / seat actions
    Play {
        #[command(subcommand)]
        cmd: PlayCmd,
    },
    /// Full-table automation via subprocess CLI only (see `simulate cliplay`)
    Simulate {
        #[command(subcommand)]
        cmd: SimulateCmd,
    },
    /// Show embedded reference material (no server required)
    Show {
        #[command(subcommand)]
        cmd: ShowCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum ShowCmd {
    /// Print concise Guan Dan rules (Markdown) to stdout
    Rules {
        /// Language code: en or zh (default: en)
        #[arg(long, default_value = "en")]
        lang: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum SimulateCmd {
    /// Simulate via CLI subprocesses. Optionally target an existing table; otherwise create one.
    Cliplay {
        /// Optional existing table ID to join. If omitted, creates a fresh table.
        #[arg(short = 't', long)]
        table: Option<String>,
        /// Starting rank/level for table creation (2-10, J, Q, K, A). Only valid when creating a new table.
        #[arg(long)]
        rank: Option<String>,
        /// Number of bots to join. If omitted, auto-fills all current vacancies.
        #[arg(long)]
        players: Option<u8>,
        /// Number of hands to complete (each ends in scoring)
        #[arg(long, default_value_t = 1)]
        hands: u32,
    },
}

#[derive(Subcommand)]
pub(crate) enum ServerCmd {
    /// Run the Axum server in the foreground
    Serve {
        /// Bind IP (default: 0.0.0.0)
        #[arg(long, default_value = "0.0.0.0")]
        ip: IpAddr,
        /// Server port (default: env `PORT`, or 22222)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Spawn local `127.0.0.1:22222` in the background (if not already running)
    #[command(visible_alias = "new")]
    Start {
        /// Do not update config `server_url` to this server after success
        #[arg(long)]
        no_auto_use: bool,
    },
    /// Stop the process serving `127.0.0.1:22222` (PID from GET /ping there; ignores config `server_url`)
    Stop,
    /// Stop then start that local server (`127.0.0.1:22222`)
    Restart {
        /// Do not update config `server_url` after a successful start
        #[arg(long)]
        no_auto_use: bool,
    },
    /// Set active server URL
    Use { host_or_port: String },
    /// Probe configured server and print config summary
    Status,
}

#[derive(Subcommand)]
pub(crate) enum TableCmd {
    /// List tables on the active server (default omits `hand`; use `--detail` for full public state)
    List {
        /// Include `hand` in each table state (same as observer snapshot)
        #[arg(long)]
        detail: bool,
    },
    /// Create a table (name is optional metadata for humans)
    Create {
        name: Option<String>,
        /// Starting rank/level (2-10, J, Q, K, A). Default: 2.
        #[arg(long)]
        rank: Option<String>,
    },
    /// Join a table
    Join {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        r#type: Option<String>,
        /// AI model name. Effective only when `--type ai`.
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value = "auto")]
        seat: String,
        /// Skip `table sync` after join (default: sync updates session; stdout is join API JSON only)
        #[arg(long)]
        no_sync: bool,
    },
    /// Long-poll for the next transition (`sinceSeq + 1`)
    Nextstate {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: Option<String>,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
        #[arg(long, default_value_t = 60000)]
        timeout_ms: u64,
    },
    /// Current table snapshot (GET snapshot)
    Snapshot {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: Option<String>,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
    },
    /// Poll `nextstate` with `timeoutMs=0` until caught up; print materialized state (default: summary JSON).
    /// Omit `-p` for observer mode (session key `<hostPortKey>.<table_id>.observer`, same layout as players).
    Sync {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: Option<String>,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
        /// Print full table state + private (default is a fixed-key summary)
        #[arg(long)]
        full: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum PlayCmd {
    /// Mark ready (ready=true)
    Ready {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        /// Skip `table sync` after ready (default: sync updates session; stdout is ready API JSON only)
        #[arg(long)]
        no_sync: bool,
    },
    /// Long-poll until this player must act; print materialized state (default: summary JSON)
    Wait4myturn {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long, default_value_t = 60000)]
        timeout_ms: u64,
        #[arg(long)]
        seq: Option<u64>,
        /// Print full table state + private (default is a fixed-key summary)
        #[arg(long)]
        full: bool,
    },
    /// Submit tribute card
    Tribute {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
        card: String,
    },
    /// Submit return card
    Returncard {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
        card: String,
    },
    /// Submit play cards
    Playcards {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
        cards: String,
        /// Comma-separated wild target card symbols (optional; for declared wild mapping)
        #[arg(long)]
        wild_targets: Option<String>,
    },
    /// GET suggest for current actor (same as HTTP GET /suggest)
    Suggest {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        /// Omit for auto-seq from player session (`lastAppliedSeq`)
        #[arg(long)]
        seq: Option<u64>,
    },
    /// Submit pass
    Pass {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
    },
    /// Start next hand from scoring phase
    NextHand {
        #[arg(short = 't', long)]
        table_id: String,
        #[arg(short = 'p', long)]
        player_id: String,
        #[arg(short = 'k', long)]
        player_key: Option<String>,
        #[arg(long)]
        seq: Option<u64>,
    },
}

/// Everything except [`ServerCmd::Serve`], [`ServerCmd::Start`], and [`ServerCmd::Restart`]
/// (those use Tokio in `main`).
pub fn run_from_top(command: Top) -> Result<(), String> {
    match command {
        Top::Server { cmd } => {
            let cmd = cmd.unwrap_or(ServerCmd::Status);
            match cmd {
                ServerCmd::Serve { .. } | ServerCmd::Start { .. } | ServerCmd::Restart { .. } => {
                    Err("internal: Serve/Start/Restart must be started from main with a Tokio runtime"
                        .into())
                }
                ServerCmd::Stop => server_stop(),
                ServerCmd::Use { host_or_port } => server_use(host_or_port),
                ServerCmd::Status => server_status(),
            }
        }
        Top::Table { cmd } => match cmd {
            TableCmd::List { detail } => table_list(detail),
            TableCmd::Create { name, rank } => table_create(name, rank),
            TableCmd::Join {
                table_id,
                name,
                r#type,
                model,
                seat,
                no_sync,
            } => table_join(table_id, name, r#type, model, seat, no_sync),
            TableCmd::Nextstate {
                table_id,
                player_id,
                player_key,
                seq,
                timeout_ms,
            } => table_nextstate(table_id, player_id, player_key, seq, timeout_ms),
            TableCmd::Snapshot {
                table_id,
                player_id,
                player_key,
            } => table_snapshot(table_id, player_id, player_key),
            TableCmd::Sync {
                table_id,
                player_id,
                player_key,
                seq,
                full,
            } => table_sync(
                table_id,
                player_id,
                player_key,
                seq,
                Some(if full {
                    MaterializedPrintMode::Full
                } else {
                    MaterializedPrintMode::Summary
                }),
            ),
        },
        Top::Simulate { cmd } => match cmd {
            SimulateCmd::Cliplay {
                table,
                rank,
                players,
                hands,
            } => simulate_cliplay_subprocess(table, rank, players, hands),
        },
        Top::Show { cmd } => match cmd {
            ShowCmd::Rules { lang } => {
                let t = lang.trim();
                let md = rules_markdown(if t.is_empty() { None } else { Some(t) })?;
                print!("{md}");
                if !md.ends_with('\n') {
                    println!();
                }
                Ok(())
            }
        },
        Top::Play { cmd } => match cmd {
            PlayCmd::Ready {
                table_id,
                player_id,
                player_key,
                no_sync,
            } => play_ready(table_id, player_id, player_key, no_sync),
            PlayCmd::Wait4myturn {
                table_id,
                player_id,
                player_key,
                timeout_ms,
                seq,
                full,
            } => play_wait4myturn(
                table_id,
                player_id,
                player_key,
                seq,
                timeout_ms,
                if full {
                    MaterializedPrintMode::Full
                } else {
                    MaterializedPrintMode::Summary
                },
            ),
            PlayCmd::Tribute {
                table_id,
                player_id,
                player_key,
                seq,
                card,
            } => play_action(
                table_id,
                player_id,
                player_key,
                seq,
                "tribute",
                json!({ "card": card }),
            ),
            PlayCmd::Returncard {
                table_id,
                player_id,
                player_key,
                seq,
                card,
            } => play_action(
                table_id,
                player_id,
                player_key,
                seq,
                "return_card",
                json!({ "card": card }),
            ),
            PlayCmd::Playcards {
                table_id,
                player_id,
                player_key,
                seq,
                cards,
                wild_targets,
            } => {
                let cards: Vec<String> = cards
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut body = json!({ "cards": cards });
                if let Some(wt) = wild_targets {
                    let targets: Vec<String> = wt
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !targets.is_empty() {
                        body["declaredWildMapping"] = json!({ "wildTargets": targets });
                    }
                }
                play_action(table_id, player_id, player_key, seq, "play", body)
            }
            PlayCmd::Suggest {
                table_id,
                player_id,
                player_key,
                seq,
            } => play_suggest(table_id, player_id, player_key, seq.as_ref().copied()),
            PlayCmd::Pass {
                table_id,
                player_id,
                player_key,
                seq,
            } => play_action(table_id, player_id, player_key, seq, "pass", json!({})),
            PlayCmd::NextHand { .. } => {
                return Err("next_hand CLI command has been removed; hands now advance automatically after scoring".into());
            }
        },
    }
}

pub fn resolve_port(port_opt: Option<u16>) -> Result<u16, String> {
    let port = port_opt
        .or_else(|| std::env::var("PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(22222);
    Ok(port)
}

fn http_client() -> Result<Client, String> {
    // Must exceed server long-poll duration (e.g. nextstate timeoutMs) so the client does not
    // abort before the server returns 200 or 204.
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())
}

async fn probe_clawguandan_server_async(base: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/ping", normalize_base(base).trim_end_matches('/'));
    let r = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("probe failed: HTTP {}", r.status()));
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    if v.get("pong").and_then(|x| x.as_str()) != Some("clawguandan") {
        return Err("probe failed: not a clawguandan server (missing or wrong pong)".into());
    }
    Ok(())
}

pub async fn server_start(auto_use: bool) -> Result<(), String> {
    fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let local_base = normalize_base(LOCAL_SERVER_PROBE_ADDR);
    if probe_clawguandan_server_async(&local_base).await.is_ok() {
        println!("server already running at {LOCAL_SERVER_PROBE_ADDR} (detected via /ping)");
        if auto_use {
            return server_use_async(LOCAL_SERVER_PROBE_ADDR.into()).await;
        }
        return Ok(());
    }

    let default_bin = std::env::current_exe().map_err(|e| e.to_string())?;
    let server_bin = std::env::var("CLAW_GUANDAN_SERVER_BIN")
        .unwrap_or_else(|_| default_bin.to_string_lossy().to_string());

    // Self-spawn: run the same binary with `server serve`.
    let child = Command::new(&server_bin)
        .args(["server", "serve", "--port", "22222", "--ip", "0.0.0.0"])
        .env("PORT", "22222")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "failed to spawn `{}` (set CLAW_GUANDAN_SERVER_BIN): {}",
                server_bin, e
            )
        })?;

    let pid = child.id();
    println!("started {} (pid {})", server_bin, pid);
    let mut last_err = String::new();
    for attempt in 0..50 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let r = if auto_use {
            server_use_async(LOCAL_SERVER_PROBE_ADDR.into()).await
        } else {
            probe_clawguandan_server_async(&local_base).await
        };
        match r {
            Ok(()) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(format!(
        "{last_err} (server process {pid} may still be starting; retry `clawguandan server use {}` or check logs)",
        LOCAL_SERVER_PROBE_ADDR
    ))
}

fn ping_pid_blocking(base: &str) -> Result<u32, String> {
    let v = fetch_ping_json_blocking(base)?;
    let n = v
        .get("pid")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "/ping missing pid field (upgrade clawguandan server)".to_string())?;
    u32::try_from(n).map_err(|_| format!("invalid pid value {n}"))
}

#[cfg(unix)]
fn unix_signal_process(pid: u32, sig: i32) -> Result<(), String> {
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if rc == 0 {
        return Ok(());
    }
    let errno = unsafe { *libc::__errno_location() };
    if errno == libc::ESRCH {
        return Ok(());
    }
    Err(std::io::Error::from_raw_os_error(errno).to_string())
}

#[cfg(unix)]
fn unix_pid_exited(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return false;
    }
    unsafe { *libc::__errno_location() == libc::ESRCH }
}

/// Stop whatever serves [`LOCAL_SERVER_PROBE_ADDR`]: GET `/ping` there for PID (ignores config).
/// Success means `kill(pid, 0)` returns `ESRCH`.
pub fn server_stop() -> Result<(), String> {
    #[cfg(not(unix))]
    {
        return Err("server stop is only supported on Unix".into());
    }
    #[cfg(unix)]
    {
        let base = normalize_base(LOCAL_SERVER_PROBE_ADDR);
        let pid = ping_pid_blocking(&base).map_err(|e| {
            format!("cannot stop: no clawguandan server on {LOCAL_SERVER_PROBE_ADDR} ({e})")
        })?;

        unix_signal_process(pid, libc::SIGTERM).map_err(|e| format!("SIGTERM: {e}"))?;

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if unix_pid_exited(pid) {
                println!("stopped server (pid {pid})");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        unix_signal_process(pid, libc::SIGKILL).map_err(|e| format!("SIGKILL: {e}"))?;

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if unix_pid_exited(pid) {
                println!("stopped server (pid {pid}) after SIGKILL");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "server pid {pid} did not exit; check permissions or process state"
        ))
    }
}

/// True when nothing answers on [`LOCAL_SERVER_PROBE_ADDR`] — failed `server_stop` is benign for `restart`.
fn restart_may_ignore_failed_stop() -> bool {
    let base = normalize_base(LOCAL_SERVER_PROBE_ADDR);
    probe_clawguandan_server(&base).is_err()
}

pub async fn server_restart(auto_use: bool) -> Result<(), String> {
    if let Err(e) = server_stop() {
        if !restart_may_ignore_failed_stop() {
            return Err(e);
        }
        println!(
            "note: stop failed but {LOCAL_SERVER_PROBE_ADDR} is unreachable; continuing restart ({e})"
        );
    }
    server_start(auto_use).await
}

fn persist_active_server(base: String) -> Result<(), String> {
    let mut cfg = CliConfig::load();
    cfg.server_url = Some(base);
    cfg.save()?;
    println!("active server: {}", cfg.server_url.as_deref().unwrap_or(""));
    Ok(())
}

fn server_use(host_or_port: String) -> Result<(), String> {
    let base = normalize_base(&host_or_port);
    probe_clawguandan_server(&base)?;
    persist_active_server(base)
}

/// Like [`server_use`], but for callers already inside a Tokio runtime (e.g. `server start`).
/// Blocking `reqwest` must not be used there: its client builds a nested runtime and panics on drop.
async fn server_use_async(host_or_port: String) -> Result<(), String> {
    let base = normalize_base(&host_or_port);
    probe_clawguandan_server_async(&base).await?;
    persist_active_server(base)
}

fn server_status() -> Result<(), String> {
    try_autoconfigure_local_server()?;
    let cfg = CliConfig::load();
    println!("server_url: {:?}", cfg.server_url);
    let status = match &cfg.server_url {
        None => "unreachable",
        Some(url) => match ping_clawguandan_info(url) {
            Ok(_) => "active",
            Err(_) => "unreachable",
        },
    };
    println!("status: {status}");
    Ok(())
}

fn table_list(detail: bool) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let mut u = Url::parse(&format!("{}/api/v1/tables", base.trim_end_matches('/')))
        .map_err(|e| e.to_string())?;
    if detail {
        u.query_pairs_mut().append_pair("detail", "true");
    }
    let r = client.get(u.as_str()).send().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("list failed: {}", r.status()));
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn table_create(name: Option<String>, rank: Option<String>) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let body = json!({ "name": name, "rank": rank });
    let r = client
        .post(format!("{}/api/v1/tables", base))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("create failed: {}", r.status()));
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn parse_player_type(s: Option<String>) -> Result<Option<String>, String> {
    Ok(match s.as_deref() {
        None | Some("") => None,
        Some("human") => Some("human".into()),
        Some("ai") => Some("ai".into()),
        Some("unknown") => Some("unknown".into()),
        Some(x) => return Err(format!("invalid player type {:?}", x)),
    })
}

fn table_snapshot(
    table_id: String,
    player_id: Option<String>,
    player_key: Option<String>,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let mut u = Url::parse(&format!(
        "{}/api/v1/tables/{}/snapshot",
        base.trim_end_matches('/'),
        table_id
    ))
    .map_err(|e| e.to_string())?;
    if let Some(pid) = &player_id {
        let pkey = resolve_player_key(&base, &table_id, pid, player_key.as_deref())?;
        u.query_pairs_mut().append_pair("playerId", pid);
        u.query_pairs_mut().append_pair("playerKey", &pkey);
    }
    let r = client.get(u).send().map_err(|e| e.to_string())?;
    let status = r.status();
    if !status.is_success() {
        let v: serde_json::Value = r.json().unwrap_or(json!({}));
        return Err(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!("snapshot {}", status))
        );
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn cli_needs_my_action(state: &TableState, player_id: &str) -> bool {
    let exp = &state.expect;
    let actor_match = exp.actor_player_ids.iter().any(|id| id == player_id);
    if !actor_match {
        return false;
    }
    match exp.kind.as_str() {
        "join" | "wait" | "game_over" => false,
        _ => !exp.legal_actions.is_empty(),
    }
}

/// Game ended: `wait4myturn` should exit with current materialized state (no long-poll).
fn cli_table_is_terminal(st: &TableState) -> bool {
    matches!(st.status, TableStatus::Finished) || st.expect.kind == "game_over"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaterializedPrintMode {
    Summary,
    Full,
}

fn key_suffix_triggers_compact_array(key: &str) -> bool {
    key.ends_with("cards")
        || key.ends_with("Cards")
        || key.ends_with("seats")
        || key.ends_with("Seats")
}

/// Pretty JSON for stdout: normal indentation, except object keys ending in
/// `cards`/`Cards`/`seats`/`Seats` whose value is an array are rendered with `to_string` (one line).
fn format_json_pretty_compact_suffix_arrays(value: &Value) -> Result<String, String> {
    let mut out = String::new();
    fmt_value_pretty_compact(value, &mut out, 0)?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn fmt_value_pretty_compact(v: &Value, out: &mut String, depth: usize) -> Result<(), String> {
    let pad = |d: usize| "  ".repeat(d);
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            let enc = serde_json::to_string(&Value::String(s.clone())).map_err(|e| e.to_string())?;
            out.push_str(&enc);
        }
        Value::Array(arr) => {
            out.push('[');
            if arr.is_empty() {
                out.push(']');
                return Ok(());
            }
            out.push('\n');
            for (i, item) in arr.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                fmt_value_pretty_compact(item, out, depth + 1)?;
                if i + 1 < arr.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            if map.is_empty() {
                out.push('}');
                return Ok(());
            }
            out.push('\n');
            let entries: Vec<(&String, &Value)> = map.iter().collect();
            for (i, (k, val)) in entries.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                let key_json =
                    serde_json::to_string(&Value::String((*k).to_string())).map_err(|e| e.to_string())?;
                out.push_str(&key_json);
                out.push_str(": ");
                if key_suffix_triggers_compact_array(k) && val.is_array() {
                    out.push_str(&serde_json::to_string(val).map_err(|e| e.to_string())?);
                } else {
                    fmt_value_pretty_compact(val, out, depth + 1)?;
                }
                if i + 1 < entries.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
    }
    Ok(())
}

fn build_session_summary_value(session: &PlayerSession) -> Result<Value, String> {
    let st = session
        .table_state
        .as_ref()
        .ok_or_else(|| "session has no materialized table_state".to_string())?;

    let hand_out = match &st.hand {
        None => Value::Null,
        Some(v) if v.is_null() => Value::Null,
        Some(v) => {
            let top = v.get("topPlay").cloned().unwrap_or(Value::Null);
            let hand_level = v.get("handLevel").cloned().unwrap_or(Value::Null);
            json!({ "topPlay": top, "handLevel": hand_level })
        }
    };

    let private_out = match &session.private_view {
        None => Value::Null,
        Some(pv) => serde_json::to_value(pv).map_err(|e| e.to_string())?,
    };

    Ok(json!({
        "seq": st.seq,
        "status": &st.status,
        "phase": &st.phase,
        "expect": &st.expect,
        "narration": &st.narration,
        "hand": hand_out,
        "private": private_out,
    }))
}

fn build_session_full_value(session: &PlayerSession) -> Result<Value, String> {
    let st = session
        .table_state
        .as_ref()
        .ok_or_else(|| "session has no materialized table_state".to_string())?;
    let mut m = serde_json::to_value(st).map_err(|e| e.to_string())?;
    if let Some(ref pv) = session.private_view {
        if let Some(obj) = m.as_object_mut() {
            obj.insert(
                "private".into(),
                serde_json::to_value(pv).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(m)
}

fn print_materialized_session(session: &PlayerSession, mode: MaterializedPrintMode) -> Result<(), String> {
    let v = match mode {
        MaterializedPrintMode::Summary => build_session_summary_value(session)?,
        MaterializedPrintMode::Full => build_session_full_value(session)?,
    };
    println!("{}", format_json_pretty_compact_suffix_arrays(&v)?);
    Ok(())
}

fn http_get_snapshot_parsed(
    base: &str,
    client: &Client,
    table_id: &str,
    player_id: Option<&str>,
    player_key: Option<&str>,
) -> Result<SnapshotApiBody, String> {
    let mut u = Url::parse(&format!(
        "{}/api/v1/tables/{}/snapshot",
        base.trim_end_matches('/'),
        table_id
    ))
    .map_err(|e| e.to_string())?;
    if let Some(pid) = player_id {
        u.query_pairs_mut().append_pair("playerId", pid);
        let pkey = player_key
            .ok_or_else(|| "playerKey is required when playerId is set".to_string())?;
        u.query_pairs_mut().append_pair("playerKey", pkey);
    }
    let r = client.get(u).send().map_err(|e| e.to_string())?;
    let status = r.status();
    if !status.is_success() {
        let v: serde_json::Value = r.json().unwrap_or(json!({}));
        return Err(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!("snapshot {}", status))
        );
    }
    r.json::<SnapshotApiBody>().map_err(|e| e.to_string())
}

fn ensure_session_bootstrap(
    base: &str,
    client: &Client,
    table_id: &str,
    player_id: &str,
    player_key: &str,
) -> Result<PlayerSession, String> {
    let mut s = read_player_session(base, table_id, player_id)?.unwrap_or_default();
    if s.table_state.is_none() {
        let snap = http_get_snapshot_parsed(base, client, table_id, Some(player_id), Some(player_key))?;
        s.table_state = Some(snap.state);
        s.private_view = snap.private;
        s.last_applied_seq = s.table_state.as_ref().map(|t| t.seq).unwrap_or(0);
        write_player_session(base, table_id, player_id, &s)?;
    }
    Ok(s)
}

fn merge_nextstate_into_session(
    base: &str,
    client: &Client,
    table_id: &str,
    player_id: &str,
    player_key: &str,
    body: &NextStateBody,
) -> Result<(), String> {
    let mut s = ensure_session_bootstrap(base, client, table_id, player_id, player_key)?;
    let ts = s
        .table_state
        .as_mut()
        .ok_or_else(|| "bootstrap left table_state empty".to_string())?;
    let new_ts = apply_transition_delta_to_table_state(ts, &body.transition.delta)
        .map_err(|e| format!("apply transition delta: {e}"))?;
    *ts = new_ts;
    s.last_applied_seq = body.transition.seq;
    s.private_view = body.private.clone();
    write_player_session(base, table_id, player_id, &s)?;
    Ok(())
}

fn ensure_session_bootstrap_observer(
    base: &str,
    client: &Client,
    table_id: &str,
) -> Result<PlayerSession, String> {
    let mut s = read_observer_session(base, table_id)?.unwrap_or_default();
    if s.table_state.is_none() {
        let snap = http_get_snapshot_parsed(base, client, table_id, None, None)?;
        s.table_state = Some(snap.state);
        s.private_view = None;
        s.last_applied_seq = s.table_state.as_ref().map(|t| t.seq).unwrap_or(0);
        write_observer_session(base, table_id, &s)?;
    }
    Ok(s)
}

fn merge_nextstate_into_observer_session(
    base: &str,
    client: &Client,
    table_id: &str,
    body: &NextStateBody,
) -> Result<(), String> {
    let mut s = ensure_session_bootstrap_observer(base, client, table_id)?;
    let ts = s
        .table_state
        .as_mut()
        .ok_or_else(|| "bootstrap left table_state empty".to_string())?;
    let new_ts = apply_transition_delta_to_table_state(ts, &body.transition.delta)
        .map_err(|e| format!("apply transition delta: {e}"))?;
    *ts = new_ts;
    s.last_applied_seq = body.transition.seq;
    s.private_view = None;
    write_observer_session(base, table_id, &s)?;
    Ok(())
}

fn play_suggest(
    table_id: String,
    player_id: String,
    player_key: Option<String>,
    manual_seq: Option<u64>,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let pkey = resolve_player_key(&base, &table_id, &player_id, player_key.as_deref())?;
    let seq = if let Some(s) = manual_seq {
        s
    } else {
        read_session_last_applied_seq(&base, &table_id, &player_id)?
            .ok_or_else(|| {
                "auto-seq: no stored lastAppliedSeq for this player; run `table sync` or `table nextstate` with `-p` first, or pass `--seq`"
                    .to_string()
            })?
    };
    let mut u = Url::parse(&format!(
        "{}/api/v1/tables/{}/suggest",
        base.trim_end_matches('/'),
        table_id
    ))
    .map_err(|e| e.to_string())?;
    u.query_pairs_mut()
        .append_pair("seq", &seq.to_string())
        .append_pair("playerId", &player_id)
        .append_pair("playerKey", &pkey);
    let r = client.get(u.as_str()).send().map_err(|e| e.to_string())?;
    let status = r.status();
    if !status.is_success() {
        let v: serde_json::Value = r.json().unwrap_or(json!({}));
        return Err(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!("suggest {}", status))
        );
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn table_join(
    table_id: String,
    name: String,
    player_type: Option<String>,
    model: Option<String>,
    seat: String,
    no_sync: bool,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let pt = parse_player_type(player_type)?;
    let mut body = json!({
        "playerName": name,
        "seat": seat,
    });
    if let Some(t) = pt {
        body["playerType"] = json!(t);
    }
    if let Some(m) = model {
        body["playerModel"] = json!(m);
    }
    let r = client
        .post(format!("{}/api/v1/tables/{}/join", base, table_id))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("join failed: {}", r.status()));
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    let pid = v["playerId"]
        .as_str()
        .ok_or_else(|| "join: missing playerId".to_string())?
        .to_string();
    let pkey = v["playerKey"]
        .as_str()
        .ok_or_else(|| "join: missing playerKey".to_string())?
        .to_string();
    write_auth_session(
        &base,
        &table_id,
        &AuthSession {
            version: 1,
            player_id: pid.clone(),
            player_key: pkey.clone(),
        },
    )?;
    if no_sync {
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    table_sync(table_id, Some(pid), Some(pkey), None, None)
}

fn table_nextstate(
    table_id: String,
    player_id: Option<String>,
    player_key: Option<String>,
    manual_seq: Option<u64>,
    timeout_ms: u64,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;

    let since_seq = if let Some(s) = manual_seq {
        s
    } else if let Some(ref pid) = player_id {
        read_session_last_applied_seq(&base, &table_id, pid)?.unwrap_or(0)
    } else {
        // Observer auto-seq: read `…tableId.observer` session (same as `table sync` without `-p`).
        read_session_last_applied_seq_observer(&base, &table_id)?.unwrap_or(0)
    };

    let mut u = url::Url::parse(&format!("{}/api/v1/tables/{}/nextstate", base, table_id))
        .map_err(|e| e.to_string())?;
    u.query_pairs_mut()
        .append_pair("sinceSeq", &since_seq.to_string())
        .append_pair("timeoutMs", &timeout_ms.to_string());
    if let Some(pid) = &player_id {
        let pkey = resolve_player_key(&base, &table_id, pid, player_key.as_deref())?;
        u.query_pairs_mut().append_pair("playerId", pid);
        u.query_pairs_mut().append_pair("playerKey", &pkey);
    }

    let r = client.get(u).send().map_err(|e| e.to_string())?;
    match r.status() {
        StatusCode::NO_CONTENT => {
            println!("(no new transition within timeout)");
            Ok(())
        }
        s if s.is_success() => {
            let body: NextStateBody = r.json().map_err(|e| e.to_string())?;
            let v = serde_json::to_value(&body).map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
            );

            if let Some(pid) = &player_id
                && manual_seq.is_none()
            {
                let pkey = resolve_player_key(&base, &table_id, pid, player_key.as_deref())?;
                merge_nextstate_into_session(&base, &client, &table_id, pid, &pkey, &body)?;
            } else if player_id.is_none() && manual_seq.is_none() {
                merge_nextstate_into_observer_session(&base, &client, &table_id, &body)?;
            }
            Ok(())
        }
        _ => Err(format!("nextstate failed: {}", r.status())),
    }
}

fn table_sync(
    table_id: String,
    player_id: Option<String>,
    player_key: Option<String>,
    manual_seq: Option<u64>,
    print: Option<MaterializedPrintMode>,
) -> Result<(), String> {
    if manual_seq.is_some() {
        return Err("table sync does not support --seq (uses session auto-seq)".into());
    }
    let base = load_active_server_base()?;
    let client = http_client()?;

    // Each request uses timeoutMs=0: server returns 204 immediately when already at head.
    const NEXTSTATE_TIMEOUT_MS: u64 = 0;

    match &player_id {
        None => {
            ensure_session_bootstrap_observer(&base, &client, &table_id)?;
            loop {
                let since_seq =
                    read_session_last_applied_seq_observer(&base, &table_id)?.unwrap_or(0);
                let mut u =
                    url::Url::parse(&format!("{}/api/v1/tables/{}/nextstate", base, table_id))
                        .map_err(|e| e.to_string())?;
                u.query_pairs_mut()
                    .append_pair("sinceSeq", &since_seq.to_string())
                    .append_pair("timeoutMs", &NEXTSTATE_TIMEOUT_MS.to_string());

                let r = client.get(u).send().map_err(|e| e.to_string())?;
                match r.status() {
                    StatusCode::NO_CONTENT => {
                        break;
                    }
                    s if s.is_success() => {
                        let body: NextStateBody = r.json().map_err(|e| e.to_string())?;
                        merge_nextstate_into_observer_session(&base, &client, &table_id, &body)?;
                        if body.lag == 0 {
                            break;
                        }
                    }
                    _ => return Err(format!("nextstate failed: {}", r.status())),
                }
            }

            let s = read_observer_session(&base, &table_id)?
                .ok_or_else(|| "sync: missing session".to_string())?;
            if let Some(mode) = print {
                print_materialized_session(&s, mode)?;
            }
            Ok(())
        }
        Some(pid) => {
            let pkey = resolve_player_key(&base, &table_id, pid, player_key.as_deref())?;
            ensure_session_bootstrap(&base, &client, &table_id, pid, &pkey)?;
            loop {
                let since_seq = read_session_last_applied_seq(&base, &table_id, pid)?.unwrap_or(0);
                let mut u =
                    url::Url::parse(&format!("{}/api/v1/tables/{}/nextstate", base, table_id))
                        .map_err(|e| e.to_string())?;
                u.query_pairs_mut()
                    .append_pair("sinceSeq", &since_seq.to_string())
                    .append_pair("timeoutMs", &NEXTSTATE_TIMEOUT_MS.to_string());
                u.query_pairs_mut().append_pair("playerId", pid);
                u.query_pairs_mut().append_pair("playerKey", &pkey);

                let r = client.get(u).send().map_err(|e| e.to_string())?;
                match r.status() {
                    StatusCode::NO_CONTENT => {
                        break;
                    }
                    s if s.is_success() => {
                        let body: NextStateBody = r.json().map_err(|e| e.to_string())?;
                        merge_nextstate_into_session(&base, &client, &table_id, pid, &pkey, &body)?;
                        if body.lag == 0 {
                            break;
                        }
                    }
                    _ => return Err(format!("nextstate failed: {}", r.status())),
                }
            }

            let s = read_player_session(&base, &table_id, pid)?
                .ok_or_else(|| "sync: missing session".to_string())?;
            if let Some(mode) = print {
                print_materialized_session(&s, mode)?;
            }
            Ok(())
        }
    }
}

fn play_wait4myturn(
    table_id: String,
    player_id: String,
    player_key: Option<String>,
    manual_seq: Option<u64>,
    timeout_ms: u64,
    print_mode: MaterializedPrintMode,
) -> Result<(), String> {
    if manual_seq.is_some() {
        return Err("play wait4myturn does not support --seq (uses session auto-seq)".into());
    }
    // Catch up to server head with timeoutMs=0 nextstate loop (no long-poll at head), so the
    // local shortcut below cannot fire on a stale session while the table has moved on.
    table_sync(
        table_id.clone(),
        Some(player_id.clone()),
        player_key.clone(),
        None,
        None,
    )?;
    let base = load_active_server_base()?;
    let client = http_client()?;
    let pkey = resolve_player_key(&base, &table_id, &player_id, player_key.as_deref())?;
    let s0 = read_player_session(&base, &table_id, &player_id)?
        .ok_or_else(|| "wait4myturn: missing session".to_string())?;
    if let Some(ref st) = s0.table_state {
        if s0.last_applied_seq == st.seq {
            if cli_needs_my_action(st, &player_id) {
                return print_materialized_session(&s0, print_mode);
            }
            if cli_table_is_terminal(st) {
                return print_materialized_session(&s0, print_mode);
            }
        }
    }

    loop {
        let since_seq = read_session_last_applied_seq(&base, &table_id, &player_id)?.unwrap_or(0);
        let mut u = url::Url::parse(&format!("{}/api/v1/tables/{}/nextstate", base, table_id))
            .map_err(|e| e.to_string())?;
        u.query_pairs_mut()
            .append_pair("sinceSeq", &since_seq.to_string())
            .append_pair("timeoutMs", &timeout_ms.to_string());
        u.query_pairs_mut().append_pair("playerId", &player_id);
        u.query_pairs_mut().append_pair("playerKey", &pkey);

        let r = client.get(u).send().map_err(|e| e.to_string())?;
        match r.status() {
            StatusCode::NO_CONTENT => {
                let s = read_player_session(&base, &table_id, &player_id)?
                    .ok_or_else(|| "wait4myturn: missing session".to_string())?;
                if let Some(ref st) = s.table_state {
                    if cli_needs_my_action(st, &player_id) {
                        return print_materialized_session(&s, print_mode);
                    }
                    if s.last_applied_seq == st.seq && cli_table_is_terminal(st) {
                        return print_materialized_session(&s, print_mode);
                    }
                }
            }
            s if s.is_success() => {
                let body: NextStateBody = r.json().map_err(|e| e.to_string())?;
                merge_nextstate_into_session(&base, &client, &table_id, &player_id, &pkey, &body)?;
                let s = read_player_session(&base, &table_id, &player_id)?
                    .ok_or_else(|| "wait4myturn: missing session".to_string())?;
                if let Some(ref st) = s.table_state {
                    if cli_needs_my_action(st, &player_id) {
                        return print_materialized_session(&s, print_mode);
                    }
                    if s.last_applied_seq == st.seq && cli_table_is_terminal(st) {
                        return print_materialized_session(&s, print_mode);
                    }
                }
            }
            _ => return Err(format!("nextstate failed: {}", r.status())),
        }
    }
}

fn play_ready(
    table_id: String,
    player_id: String,
    player_key: Option<String>,
    no_sync: bool,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let pkey = resolve_player_key(&base, &table_id, &player_id, player_key.as_deref())?;

    let body = json!({
        "playerId": player_id,
        "playerKey": pkey,
        "ready": true,
    });
    let r = client
        .post(format!("{}/api/v1/tables/{}/ready", base, table_id))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let status = r.status();
    if !status.is_success() {
        let v: serde_json::Value = r.json().unwrap_or(json!({}));
        return Err(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!("ready {}", status))
        );
    }
    let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    if no_sync {
        return Ok(());
    }
    table_sync(table_id, Some(player_id), player_key, None, None)
}

fn play_action(
    table_id: String,
    player_id: String,
    player_key: Option<String>,
    manual_seq: Option<u64>,
    action: &str,
    mut payload: serde_json::Value,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;
    let pkey = resolve_player_key(&base, &table_id, &player_id, player_key.as_deref())?;
    let mut retried_after_stale_seq = false;
    loop {
        let seq = if let Some(s) = manual_seq {
            s
        } else {
            read_session_last_applied_seq(&base, &table_id, &player_id)?
                .ok_or_else(|| {
                    "auto-seq: no stored lastAppliedSeq for this player; run `table nextstate` first or pass `--seq`".to_string()
                })?
        };
        payload["playerId"] = json!(&player_id);
        payload["playerKey"] = json!(&pkey);
        payload["seq"] = json!(seq);
        let r = client
            .post(format!(
                "{}/api/v1/tables/{}/actions/{}",
                base, table_id, action
            ))
            .json(&payload)
            .send()
            .map_err(|e| e.to_string())?;
        let status = r.status();
        if status.is_success() {
            let v: serde_json::Value = r.json().map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
            );
            if manual_seq.is_none() {
                table_sync(
                    table_id,
                    Some(player_id),
                    Some(pkey.clone()),
                    None,
                    None,
                )?;
            }
            return Ok(());
        }
        let v: serde_json::Value = r.json().unwrap_or(json!({}));
        let code = v
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_str());
        let recoverable_stale_seq = status == StatusCode::CONFLICT
            && manual_seq.is_none()
            && !retried_after_stale_seq
            && code == Some("STALE_SEQ");
        if recoverable_stale_seq {
            table_sync(
                table_id.clone(),
                Some(player_id.clone()),
                Some(pkey.clone()),
                None,
                None,
            )?;
            retried_after_stale_seq = true;
            continue;
        }
        return Err(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!("{} failed", action))
        );
    }
}

fn parse_cli_stdout_json(out: &[u8]) -> Result<serde_json::Value, String> {
    let s = String::from_utf8_lossy(out);
    let t = s.trim();
    serde_json::from_str(t).map_err(|e| format!("invalid JSON from CLI: {e}; got: {t:?}"))
}

fn nextstate_stdout_is_no_content(stdout: &[u8]) -> bool {
    let t = String::from_utf8_lossy(stdout).trim().to_string();
    t.is_empty() || t.starts_with("(no new transition within timeout)")
}

fn transition_counts_as_hand_done(v: &serde_json::Value) -> bool {
    matches!(
        v.get("type").and_then(|x| x.as_str()),
        Some("HAND_ENDED_WAITING_READY" | "GAME_COMPLETED")
    )
}

fn table_state_is_terminal(v: &serde_json::Value) -> bool {
    v.get("status").and_then(|x| x.as_str()) == Some("finished")
        || v.get("expect")
            .and_then(|e| e.get("kind"))
            .and_then(|x| x.as_str())
            == Some("game_over")
}

fn expect_requires_action(state: &serde_json::Value, my_pid: &str) -> bool {
    let expect = state.get("expect").unwrap_or(&serde_json::Value::Null);
    let kind = expect.get("kind").and_then(|x| x.as_str()).unwrap_or("");
    let actor_match = expect
        .get("actorPlayerIds")
        .and_then(|x| x.as_array())
        .map(|ids| ids.iter().any(|id| id.as_str() == Some(my_pid)))
        .unwrap_or(false);
    if !actor_match {
        return false;
    }
    match kind {
        "play" | "tribute" | "exchange" | "ready" => true,
        _ => false,
    }
}

fn expect_has_uncontrolled_actor(
    expect: &serde_json::Value,
    controlled_pids: &HashSet<String>,
) -> Option<String> {
    let kind = expect.get("kind").and_then(|x| x.as_str()).unwrap_or("");
    if !matches!(kind, "play" | "tribute" | "exchange") {
        return None;
    }
    if let Some(ids) = expect.get("actorPlayerIds").and_then(|x| x.as_array()) {
        for id in ids {
            let Some(actor) = id.as_str() else {
                continue;
            };
            if !controlled_pids.contains(actor) {
                return Some(actor.to_string());
            }
        }
        return None;
    }
    None
}

fn player_action_to_cli_argv_auto(
    action: &PlayerAction,
    table_id: &str,
    player_id: &str,
) -> Vec<String> {
    match action {
        PlayerAction::Tribute { card } => vec![
            "play".into(),
            "tribute".into(),
            "-t".into(),
            table_id.into(),
            "-p".into(),
            player_id.into(),
            card.clone(),
        ],
        PlayerAction::ReturnCard { card } => vec![
            "play".into(),
            "returncard".into(),
            "-t".into(),
            table_id.into(),
            "-p".into(),
            player_id.into(),
            card.clone(),
        ],
        PlayerAction::Pass => vec![
            "play".into(),
            "pass".into(),
            "-t".into(),
            table_id.into(),
            "-p".into(),
            player_id.into(),
        ],
        PlayerAction::Play {
            cards,
            wild_targets,
        } => {
            let csv = cards.join(",");
            let mut v = vec![
                "play".into(),
                "playcards".into(),
                "-t".into(),
                table_id.into(),
                "-p".into(),
                player_id.into(),
                csv,
            ];
            if let Some(wt) = wild_targets {
                if !wt.is_empty() {
                    v.push("--wild-targets".into());
                    v.push(wt.join(","));
                }
            }
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expect_has_uncontrolled_actor_detects_outside_actor() {
        let expect = json!({
            "kind": "play",
            "actorPlayerIds": ["p_human"],
            "legalActions": ["play", "pass"]
        });
        let controlled = HashSet::from(["p_bot1".to_string(), "p_bot2".to_string()]);
        assert_eq!(
            expect_has_uncontrolled_actor(&expect, &controlled).as_deref(),
            Some("p_human")
        );
    }

    #[test]
    fn expect_has_uncontrolled_actor_ignores_non_action_kinds() {
        let expect = json!({
            "kind": "wait",
            "actorPlayerIds": ["p_human"],
            "legalActions": []
        });
        let controlled = HashSet::from(["p_bot1".to_string()]);
        assert!(expect_has_uncontrolled_actor(&expect, &controlled).is_none());
    }

    #[test]
    fn expect_has_uncontrolled_actor_prefers_actor_ids_collection() {
        let expect = json!({
            "kind": "play",
            "actorPlayerIds": ["p_bot1", "p_human"],
            "legalActions": ["play", "pass"]
        });
        let controlled = HashSet::from(["p_bot1".to_string()]);
        assert_eq!(
            expect_has_uncontrolled_actor(&expect, &controlled).as_deref(),
            Some("p_human")
        );
    }

    #[test]
    fn expect_has_uncontrolled_actor_ignores_ready_kind() {
        let expect = json!({
            "kind": "ready",
            "actorPlayerIds": ["p_bot1", "p_human"],
            "legalActions": ["ready"]
        });
        let controlled = HashSet::from(["p_bot1".to_string()]);
        assert!(expect_has_uncontrolled_actor(&expect, &controlled).is_none());
    }

    #[test]
    fn hand_done_transition_before_or_at_start_seq_is_ignored() {
        let shared = CliplayShared {
            stop: AtomicBool::new(false),
            start_seq: 100,
            last_scoring_transition_seq: Mutex::new(None),
            hands_done: AtomicU32::new(0),
            hands_target: 1,
            err: Mutex::new(None),
        };
        let old = json!({
            "seq": 100,
            "type": "HAND_ENDED_WAITING_READY",
            "delta": { "ops": [{ "op": "replace", "path": "/phase", "value": "scoring" }] }
        });
        shared.on_transition_maybe_scoring(&old);
        assert_eq!(shared.hands_done.load(Ordering::SeqCst), 0);

        let fresh = json!({
            "seq": 101,
            "type": "ACTION_APPLIED",
            "delta": { "ops": [{ "op": "replace", "path": "/phase", "value": "scoring" }] }
        });
        shared.on_transition_maybe_scoring(&fresh);
        assert_eq!(shared.hands_done.load(Ordering::SeqCst), 0);

        let hand_done = json!({
            "seq": 102,
            "type": "HAND_ENDED_WAITING_READY",
            "delta": { "ops": [{ "op": "replace", "path": "/phase", "value": "scoring" }] }
        });
        shared.on_transition_maybe_scoring(&hand_done);
        assert_eq!(shared.hands_done.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn terminal_transition_sets_global_stop() {
        let shared = CliplayShared {
            stop: AtomicBool::new(false),
            start_seq: 0,
            last_scoring_transition_seq: Mutex::new(None),
            hands_done: AtomicU32::new(0),
            hands_target: 99,
            err: Mutex::new(None),
        };
        let terminal = json!({
            "seq": 201,
            "type": "GAME_COMPLETED",
            "delta": { "ops": [{ "op": "replace", "path": "/status", "value": "finished" }] }
        });
        shared.on_transition_maybe_terminal(&terminal);
        assert!(shared.stop.load(Ordering::SeqCst));
    }
}

struct CliplayShared {
    stop: AtomicBool,
    start_seq: u64,
    last_scoring_transition_seq: Mutex<Option<u64>>,
    hands_done: AtomicU32,
    hands_target: u32,
    err: Mutex<Option<String>>,
}

impl CliplayShared {
    fn fail(&self, msg: String) {
        let mut e = self.err.lock().unwrap();
        if e.is_none() {
            *e = Some(msg);
        }
        self.stop.store(true, Ordering::SeqCst);
    }

    fn on_transition_maybe_scoring(&self, v: &serde_json::Value) {
        if !transition_counts_as_hand_done(v) {
            return;
        }
        let Some(tr_seq) = v.get("seq").and_then(|x| x.as_u64()) else {
            return;
        };
        if tr_seq <= self.start_seq {
            return;
        }
        let mut last = self.last_scoring_transition_seq.lock().unwrap();
        if *last == Some(tr_seq) {
            return;
        }
        *last = Some(tr_seq);
        let n = self.hands_done.fetch_add(1, Ordering::SeqCst) + 1;
        println!("\n--- simulate cliplay: hand {n} completed (transition seq={tr_seq}) ---");
        if n >= self.hands_target {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn on_transition_maybe_terminal(&self, v: &serde_json::Value) {
        let terminal_by_type =
            v.get("type").and_then(|x| x.as_str()) == Some("GAME_COMPLETED");
        let terminal_by_delta = v
            .get("delta")
            .and_then(|d| d.get("ops"))
            .and_then(|ops| ops.as_array())
            .map(|ops| {
                ops.iter().any(|op| {
                    op.get("path").and_then(|x| x.as_str()) == Some("/status")
                        && op.get("value").and_then(|x| x.as_str()) == Some("finished")
                })
            })
            .unwrap_or(false);
        if terminal_by_type || terminal_by_delta {
            self.stop.store(true, Ordering::SeqCst);
        }
    }
}

fn simulate_cliplay_subprocess(
    table: Option<String>,
    rank: Option<String>,
    players: Option<u8>,
    hands: u32,
) -> Result<(), String> {
    if hands == 0 {
        return Err("--hands must be >= 1".into());
    }
    if let Some(n) = players
        && n > 4
    {
        return Err("--players must be <= 4".into());
    }
    let bin = std::env::current_exe().map_err(|e| e.to_string())?;

    println!("--- simulate cliplay: hands={hands} (observer + bots; subprocess CLI; auto-seq) ---");

    let table_id = if let Some(tid) = table {
        if rank.is_some() {
            return Err("--rank is only allowed when creating a new table (omit --table)".into());
        }
        println!("\n### [table target]\nusing existing table: {tid}");
        tid
    } else {
        let label = "table create";
        let mut create_args = vec![
            "table".to_string(),
            "create".to_string(),
            "simulate-cliplay".to_string(),
        ];
        if let Some(rank) = rank.as_deref() {
            create_args.push("--rank".to_string());
            create_args.push(rank.to_string());
        }
        println!("\n### [{label}]\n$ clawguandan {}", create_args.join(" "));
        let out = run_cli_command(&bin, &create_args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        println!("<< stdout:\n{stdout}");
        let create_v = parse_cli_stdout_json(&out.stdout)?;
        create_v["tableId"]
            .as_str()
            .or_else(|| create_v["table_id"].as_str())
            .ok_or_else(|| "create: missing tableId".to_string())?
            .to_string()
    };

    let snapshot_args = vec![
        "table".to_string(),
        "snapshot".to_string(),
        "-t".to_string(),
        table_id.clone(),
    ];
    println!(
        "\n### [table snapshot]\n$ clawguandan {}",
        snapshot_args.join(" ")
    );
    let snapshot_out = run_cli_command(&bin, &snapshot_args).map_err(|e| e.to_string())?;
    let snapshot_stdout = String::from_utf8_lossy(&snapshot_out.stdout);
    println!("<< stdout:\n{snapshot_stdout}");
    let snapshot = parse_cli_stdout_json(&snapshot_out.stdout)?;
    let snapshot_state: TableState =
        serde_json::from_value(snapshot.clone()).map_err(|e| format!("snapshot parse: {e}"))?;
    let start_seq = snapshot_state.seq;
    let seats = snapshot["seats"]
        .as_object()
        .ok_or_else(|| "snapshot: missing seats".to_string())?;
    let occupied = seats
        .values()
        .filter(|seat| {
            seat.get("playerId")
                .and_then(|x| x.as_str())
                .map(|x| !x.is_empty())
                .unwrap_or(false)
        })
        .count();
    if occupied > 4 {
        return Err(format!(
            "snapshot: invalid occupied seat count {occupied} (>4)"
        ));
    }
    let vacancy = 4usize.saturating_sub(occupied);
    let target_join = if let Some(n) = players {
        usize::from(n)
    } else {
        vacancy
    };
    if players.is_none() && vacancy == 0 {
        return Err(
            "no seat vacancy: omit `--table` to create a new table, or pass `--players` explicitly"
                .into(),
        );
    }
    if target_join > vacancy {
        return Err(format!(
            "requested --players {target_join}, but only {vacancy} seat(s) are available on table {table_id}"
        ));
    }
    println!(
        "\n--- simulate cliplay: table_id={table_id} occupied={occupied} vacancy={vacancy} target_join={target_join} ---"
    );

    // Reset observer auto-seq baseline to current head to avoid replaying historical scoring
    // transitions from a stale persisted `<table>.observer` session.
    let base = load_active_server_base()?;
    write_observer_session(
        &base,
        &table_id,
        &PlayerSession {
            version: 1,
            last_applied_seq: start_seq,
            table_state: Some(snapshot_state.clone()),
            private_view: None,
        },
    )?;

    let mut pids: Vec<String> = Vec::new();
    for i in 0..target_join {
        let label = format!("table join bot{i}");
        let args = vec![
            "table".to_string(),
            "join".to_string(),
            "-t".to_string(),
            table_id.clone(),
            "--name".to_string(),
            format!("bot{i}"),
            "--seat".to_string(),
            "auto".to_string(),
        ];
        println!("\n### [{label}]\n$ clawguandan {}", args.join(" "));
        let out = run_cli_command(&bin, &args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        println!("<< stdout:\n{stdout}");
        let j = parse_cli_stdout_json(&out.stdout)?;
        let pid = j["playerId"]
            .as_str()
            .ok_or_else(|| "join: missing playerId".to_string())?
            .to_string();
        pids.push(pid);
    }

    for (i, pid) in pids.iter().enumerate() {
        let label = format!("play ready bot{i}");
        let args = vec![
            "play".to_string(),
            "ready".to_string(),
            "-t".to_string(),
            table_id.clone(),
            "-p".to_string(),
            pid.clone(),
        ];
        println!("\n### [{label}]\n$ clawguandan {}", args.join(" "));
        let out = run_cli_command(&bin, &args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        println!("<< stdout:\n{stdout}");
        let _j = parse_cli_stdout_json(&out.stdout)?;
    }

    let controlled_pids: Arc<HashSet<String>> = Arc::new(pids.iter().cloned().collect());
    let controlled_pids_text = pids.join(",");

    let shared = Arc::new(CliplayShared {
        stop: AtomicBool::new(false),
        start_seq,
        last_scoring_transition_seq: Mutex::new(None),
        hands_done: AtomicU32::new(0),
        hands_target: hands,
        err: Mutex::new(None),
    });

    // Keep below http_client() timeout so long-poll can return 200/204 before reqwest aborts.
    const NEXTSTATE_TIMEOUT_MS: u64 = 110_000;
    const MAX_STEPS: u64 = 500_000;

    let mut handles = Vec::new();

    let bin_obs = bin.clone();
    let table_id_obs = table_id.clone();
    let shared_obs = Arc::clone(&shared);
    handles.push(thread::spawn(move || {
        loop {
            if shared_obs.stop.load(Ordering::SeqCst) {
                break;
            }
            let argv = vec![
                "table".to_string(),
                "nextstate".to_string(),
                "-t".to_string(),
                table_id_obs.clone(),
                "--timeout-ms".to_string(),
                NEXTSTATE_TIMEOUT_MS.to_string(),
            ];
            println!("\n### [observer] $ clawguandan {}", argv.join(" "));
            let out = match run_cli_command(&bin_obs, &argv) {
                Ok(o) => o,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("error sending request") || msg.contains("connection") {
                        std::thread::sleep(Duration::from_millis(200));
                        match run_cli_command(&bin_obs, &argv) {
                            Ok(o) => o,
                            Err(e2) => {
                                shared_obs.fail(format!("observer: nextstate: {e2}"));
                                break;
                            }
                        }
                    } else {
                        shared_obs.fail(format!("observer: nextstate: {e}"));
                        break;
                    }
                }
            };
            let out_txt = String::from_utf8_lossy(&out.stdout);
            let err_txt = String::from_utf8_lossy(&out.stderr);
            println!("<< [observer] stdout:\n{out_txt}");
            if !err_txt.trim().is_empty() {
                println!("<< [observer] stderr:\n{err_txt}");
            }

            if nextstate_stdout_is_no_content(&out.stdout) {
                continue;
            }

            let v = match parse_cli_stdout_json(&out.stdout) {
                Ok(j) => j,
                Err(e) => {
                    shared_obs.fail(format!("observer: {e}"));
                    break;
                }
            };
            shared_obs.on_transition_maybe_scoring(&v);
            shared_obs.on_transition_maybe_terminal(&v);
            if shared_obs.stop.load(Ordering::SeqCst) {
                break;
            }
        }
    }));

    for (i, pid) in pids.iter().cloned().enumerate() {
        let bin = bin.clone();
        let table_id = table_id.clone();
        let shared = Arc::clone(&shared);
        let controlled_pids = Arc::clone(&controlled_pids);
        let controlled_pids_text = controlled_pids_text.clone();
        let prefix = format!("bot{i}");
        handles.push(thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50 * i as u64));
            let mut steps: u64 = 0;
            loop {
                if shared.stop.load(Ordering::SeqCst) {
                    break;
                }
                if steps >= MAX_STEPS {
                    shared.fail(format!(
                        "{prefix}: exceeded max steps ({MAX_STEPS}); possible livelock"
                    ));
                    break;
                }
                steps += 1;

                let argv = vec![
                    "play".to_string(),
                    "wait4myturn".to_string(),
                    "-t".to_string(),
                    table_id.clone(),
                    "-p".to_string(),
                    pid.clone(),
                    "--timeout-ms".to_string(),
                    NEXTSTATE_TIMEOUT_MS.to_string(),
                ];
                println!(
                    "\n### [{prefix}] $ clawguandan {}",
                    argv.join(" ")
                );
                let out = match run_cli_command(&bin, &argv) {
                    Ok(o) => o,
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("error sending request") || msg.contains("connection") {
                            std::thread::sleep(Duration::from_millis(200));
                            match run_cli_command(&bin, &argv) {
                                Ok(o) => o,
                                Err(e2) => {
                                    shared.fail(format!("{prefix}: wait4myturn: {e2}"));
                                    break;
                                }
                            }
                        } else {
                            shared.fail(format!("{prefix}: wait4myturn: {e}"));
                            break;
                        }
                    }
                };
                let out_txt = String::from_utf8_lossy(&out.stdout);
                let err_txt = String::from_utf8_lossy(&out.stderr);
                println!("<< [{prefix}] stdout:\n{out_txt}");
                if !err_txt.trim().is_empty() {
                    println!("<< [{prefix}] stderr:\n{err_txt}");
                }

                let state = match parse_cli_stdout_json(&out.stdout) {
                    Ok(j) => j,
                    Err(e) => {
                        shared.fail(format!("{prefix}: {e}"));
                        break;
                    }
                };

                if table_state_is_terminal(&state) {
                    shared.stop.store(true, Ordering::SeqCst);
                    break;
                }

                let expect = match state.get("expect") {
                    Some(e) => e.clone(),
                    None => {
                        shared.fail(format!("{prefix}: wait4myturn: missing expect"));
                        break;
                    }
                };
                let kind = expect.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                if let Some(actor) = expect_has_uncontrolled_actor(&expect, &controlled_pids) {
                    shared.fail(format!(
                        "{prefix}: actor {actor} is not controlled by simulate cliplay (join_only mode). controlled_bot_ids=[{controlled_pids_text}]"
                    ));
                    break;
                }

                if expect_requires_action(&state, &pid) {
                    if kind == "ready" {
                        let argv = vec![
                            "play".to_string(),
                            "ready".to_string(),
                            "-t".to_string(),
                            table_id.clone(),
                            "-p".to_string(),
                            pid.clone(),
                        ];
                        println!(
                            "\n### [{prefix}] $ clawguandan {}",
                            argv.join(" ")
                        );
                        match run_cli_command(&bin, &argv) {
                            Ok(o) => {
                                println!(
                                    "<< [{prefix}] stdout:\n{}",
                                    String::from_utf8_lossy(&o.stdout)
                                );
                            }
                            Err(e) => {
                                shared.fail(format!("{prefix}: ready: {e}"));
                                break;
                            }
                        }
                    } else {
                        let sargv = vec![
                            "play".to_string(),
                            "suggest".to_string(),
                            "-t".to_string(),
                            table_id.clone(),
                            "-p".to_string(),
                            pid.clone(),
                        ];
                        println!(
                            "\n### [{prefix}] $ clawguandan {}",
                            sargv.join(" ")
                        );
                        let sug_out = match run_cli_command(&bin, &sargv) {
                            Ok(o) => o,
                            Err(e) => {
                                shared.fail(format!("{prefix}: suggest: {e}"));
                                break;
                            }
                        };
                        println!(
                            "<< [{prefix}] stdout:\n{}",
                            String::from_utf8_lossy(&sug_out.stdout)
                        );
                        let sug = match parse_cli_stdout_json(&sug_out.stdout) {
                            Ok(j) => j,
                            Err(e) => {
                                shared.fail(format!("{prefix}: {e}"));
                                break;
                            }
                        };
                        let action_type = match sug.get("actionType").and_then(|x| x.as_str()) {
                            Some(s) => s,
                            None => {
                                shared.fail(format!("{prefix}: suggest: missing actionType"));
                                break;
                            }
                        };
                        let payload = sug.get("payload").cloned().unwrap_or(json!({}));
                        let action = match PlayerAction::try_from_action_type_payload(
                            action_type,
                            &payload,
                        ) {
                            Ok(a) => a,
                            Err(e) => {
                                shared.fail(format!("{prefix}: suggest parse: {e}"));
                                break;
                            }
                        };
                        let argv = player_action_to_cli_argv_auto(&action, &table_id, &pid);
                        println!(
                            "\n### [{prefix}] $ clawguandan {}",
                            argv.join(" ")
                        );
                        match run_cli_command(&bin, &argv) {
                            Ok(o) => {
                                println!(
                                    "<< [{prefix}] stdout:\n{}",
                                    String::from_utf8_lossy(&o.stdout)
                                );
                            }
                            Err(e) => {
                                shared.fail(format!("{prefix}: action: {e}"));
                                break;
                            }
                        }
                    }
                }

                if shared.stop.load(Ordering::SeqCst) {
                    break;
                }
            }
        }));
    }

    for h in handles {
        h.join()
            .map_err(|_| "simulate cliplay: thread panicked".to_string())?;
    }

    if let Some(e) = shared.err.lock().unwrap().take() {
        return Err(e);
    }

    let base = load_active_server_base()?;
    let last_seq = read_session_last_applied_seq_observer(&base, &table_id)?.unwrap_or(0);
    println!("\n=== simulate cliplay done. table_id={table_id} observer_last_seq={last_seq} ===");
    Ok(())
}

#[cfg(test)]
mod materialized_print_tests {
    use super::*;
    use clawguandan::domain::{PlayHints, PrivateView, TableState};

    #[test]
    fn summary_value_fixed_keys_and_null_private() {
        let st: TableState = serde_json::from_value(json!({
            "tableId": "t_x",
            "seq": 7u64,
            "status": "waiting",
            "phase": "table_setup",
            "narration": "hi",
            "seats": {},
            "teams": [],
            "hand": null,
            "expect": { "kind": "join", "actorPlayerIds": [], "legalActions": [] },
            "scoreboard": {}
        }))
        .unwrap();
        let session = PlayerSession {
            version: 1,
            last_applied_seq: 7,
            table_state: Some(st),
            private_view: None,
        };
        let v = build_session_summary_value(&session).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "expect".to_string(),
                "hand".to_string(),
                "narration".to_string(),
                "phase".to_string(),
                "private".to_string(),
                "seq".to_string(),
                "status".to_string(),
            ]
        );
        assert_eq!(v["seq"], json!(7));
        assert_eq!(v["private"], Value::Null);
        assert_eq!(v["hand"], Value::Null);
    }

    #[test]
    fn summary_hand_top_play_and_hand_level() {
        let st: TableState = serde_json::from_value(json!({
            "tableId": "t_x",
            "seq": 1u64,
            "status": "in_game",
            "phase": "playing",
            "narration": "",
            "seats": {},
            "teams": [],
            "hand": { "topPlay": { "seat": "E" }, "handLevel": "5", "turnSeat": "N" },
            "expect": { "kind": "play", "actorPlayerIds": ["p"], "legalActions": ["play", "pass"] },
            "scoreboard": {}
        }))
        .unwrap();
        let session = PlayerSession {
            version: 1,
            last_applied_seq: 1,
            table_state: Some(st),
            private_view: None,
        };
        let v = build_session_summary_value(&session).unwrap();
        assert_eq!(
            v["hand"],
            json!({ "topPlay": { "seat": "E" }, "handLevel": "5" })
        );
    }

    #[test]
    fn summary_hand_missing_hand_level_is_null() {
        let st: TableState = serde_json::from_value(json!({
            "tableId": "t_x",
            "seq": 1u64,
            "status": "in_game",
            "phase": "playing",
            "narration": "",
            "seats": {},
            "teams": [],
            "hand": { "topPlay": null },
            "expect": { "kind": "play", "actorPlayerIds": ["p"], "legalActions": ["play", "pass"] },
            "scoreboard": {}
        }))
        .unwrap();
        let session = PlayerSession {
            version: 1,
            last_applied_seq: 1,
            table_state: Some(st),
            private_view: None,
        };
        let v = build_session_summary_value(&session).unwrap();
        assert_eq!(v["hand"], json!({ "topPlay": null, "handLevel": null }));
    }

    #[test]
    fn full_value_includes_table_id_and_private() {
        let st: TableState = serde_json::from_value(json!({
            "tableId": "t_full",
            "seq": 2u64,
            "status": "waiting",
            "phase": "table_setup",
            "narration": "",
            "seats": {},
            "teams": [],
            "hand": null,
            "expect": { "kind": "join", "actorPlayerIds": [], "legalActions": [] },
            "scoreboard": {}
        }))
        .unwrap();
        let pv = PrivateView {
            player_id: "p1".into(),
            seat: "E".into(),
            teammate_seat: "W".into(),
            hand_cards: vec!["♠3".into()],
            play_hints: PlayHints {
                can_play: false,
                can_pass: false,
            },
        };
        let session = PlayerSession {
            version: 1,
            last_applied_seq: 2,
            table_state: Some(st),
            private_view: Some(pv),
        };
        let v = build_session_full_value(&session).unwrap();
        assert_eq!(v["tableId"], json!("t_full"));
        assert!(v.get("private").is_some());
        assert_eq!(v["private"]["teammateSeat"], json!("W"));
    }

    #[test]
    fn pretty_compact_suffix_array_on_one_line() {
        let v = json!({
            "private": {
                "handCards": ["♠3", "♥4"],
                "id": "p1"
            },
            "teams": [
                { "teamId": "t1", "seats": ["E", "W"] }
            ]
        });
        let s = format_json_pretty_compact_suffix_arrays(&v).unwrap();
        let hand_line = s
            .lines()
            .find(|ln| ln.contains("\"handCards\""))
            .expect("handCards line");
        assert!(
            hand_line.contains("[\"♠3\",\"♥4\"]") || hand_line.contains("[\"♠3\", \"♥4\"]"),
            "expected compact array on handCards line: {hand_line:?}"
        );
        let seats_line = s
            .lines()
            .find(|ln| ln.contains("\"seats\""))
            .expect("seats line");
        assert!(
            seats_line.contains("[\"E\",\"W\"]") || seats_line.contains("[\"E\", \"W\"]"),
            "expected compact array on seats line: {seats_line:?}"
        );
    }

    #[test]
    fn pretty_non_suffix_arrays_stay_multiline() {
        let v = json!({ "legalActions": ["play", "pass"] });
        let s = format_json_pretty_compact_suffix_arrays(&v).unwrap();
        assert!(s.contains("\"play\""));
        assert!(s.lines().filter(|ln| ln.trim() == "\"play\",").count() >= 1);
    }
}
