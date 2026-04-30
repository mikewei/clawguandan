use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;
use std::fs;
use std::net::IpAddr;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use url::Url;

#[path = "platform_process.rs"]
mod platform_process;

use clawguandan::bot::plugins::{
    BeatItPlugin, LlmBotParams, LlmBotPlugin, RuleBotPlugin, resolve_join_model,
    verify_script_model,
};
use clawguandan::bot::{BotRunOptions, run_bot_subprocess};
use clawguandan::domain::{
    NextStateBody, PrivateView, TableState, TableStatus, apply_transition_delta_to_table_state,
};
#[cfg(test)]
use clawguandan::game::engine::PlayerAction;
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

/// Per-session CLI state under `std::env::temp_dir()/clawguandan/` (see `player_session_dir` /
/// `observer_session_dir`). `hostPortKey` is derived from the active `server_url`.
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

fn player_session_dir(base: &str, table_id: &str, player_id: &str) -> Result<PathBuf, String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    validate_session_id_component(player_id, "player_id")?;
    Ok(session_state_root()
        .join(sid)
        .join(table_id)
        .join(player_id))
}

fn observer_session_dir(
    base: &str,
    table_id: &str,
    observer_name: &str,
) -> Result<PathBuf, String> {
    let sid = session_host_port_key_from_base(base)?;
    validate_session_id_component(table_id, "table_id")?;
    let name = observer_name.trim();
    if name.is_empty() {
        return Err("invalid observer_name: empty".into());
    }
    validate_session_id_component(name, "observer_name")?;
    Ok(session_state_root()
        .join(sid)
        .join(table_id)
        .join(format!("observer.{name}")))
}

fn player_session_json_path(
    base: &str,
    table_id: &str,
    player_id: &str,
) -> Result<PathBuf, String> {
    Ok(player_session_dir(base, table_id, player_id)?.join("session.json"))
}

fn player_auth_json_path(base: &str, table_id: &str, player_id: &str) -> Result<PathBuf, String> {
    Ok(player_session_dir(base, table_id, player_id)?.join("auth.json"))
}

fn observer_session_json_path(
    base: &str,
    table_id: &str,
    observer_name: &str,
) -> Result<PathBuf, String> {
    Ok(observer_session_dir(base, table_id, observer_name)?.join("session.json"))
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

fn read_player_session(
    base: &str,
    table_id: &str,
    player_id: &str,
) -> Result<Option<PlayerSession>, String> {
    let path = player_session_json_path(base, table_id, player_id)?;
    if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let state: PlayerSession = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        return Ok(Some(state));
    }
    Ok(None)
}

fn write_player_session(
    base: &str,
    table_id: &str,
    player_id: &str,
    session: &PlayerSession,
) -> Result<(), String> {
    let path = player_session_json_path(base, table_id, player_id)?;
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

fn read_auth_session(
    base: &str,
    table_id: &str,
    player_id: &str,
) -> Result<Option<AuthSession>, String> {
    let path = player_auth_json_path(base, table_id, player_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let auth: AuthSession = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(Some(auth))
}

fn write_auth_session(base: &str, table_id: &str, auth: &AuthSession) -> Result<(), String> {
    validate_session_id_component(table_id, "table_id")?;
    validate_session_id_component(&auth.player_id, "player_id")?;
    let path = player_auth_json_path(base, table_id, &auth.player_id)?;
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
        "missing playerKey for this player; run `table join` again or pass `--player-key`"
            .to_string()
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

fn read_observer_session(
    base: &str,
    table_id: &str,
    observer_name: &str,
) -> Result<Option<PlayerSession>, String> {
    let path = observer_session_json_path(base, table_id, observer_name)?;
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
    observer_name: &str,
    session: &PlayerSession,
) -> Result<(), String> {
    let path = observer_session_json_path(base, table_id, observer_name)?;
    let s = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    write_session_state_atomic(&path, &s)
}

fn read_session_last_applied_seq_observer(
    base: &str,
    table_id: &str,
    observer_name: &str,
) -> Result<Option<u64>, String> {
    Ok(read_observer_session(base, table_id, observer_name)?.map(|s| s.last_applied_seq))
}

fn base_url(cfg: &CliConfig) -> Result<String, String> {
    cfg.server_url.clone().ok_or_else(|| {
        err_with_hints(
            "no active server",
            &[
                "run `clawguandan server start` for local default server",
                "or run `clawguandan server use <hostOrIp[:port]>`",
            ],
        )
    })
}

/// Stable origin key for dedup (`http`/`https`, host case-folded for domains, port).
fn web_origin_key(base: &str) -> Option<String> {
    let n = normalize_base(base);
    let u = Url::parse(&n).ok()?;
    let scheme = u.scheme().to_ascii_lowercase();
    let host_key = match u.host()? {
        url::Host::Domain(d) => d.to_ascii_lowercase(),
        url::Host::Ipv4(ip) => format!("{ip}"),
        url::Host::Ipv6(ip) => format!("{ip}"),
    };
    let port = u.port_or_known_default()?;
    Some(format!("{scheme}://{host_key}:{port}"))
}

fn http_origin_is_localhost(base: &str) -> bool {
    let n = normalize_base(base);
    let Ok(u) = Url::parse(&n) else {
        return false;
    };
    match u.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// `0` = LAN-ish (private / link-local / ULA / `.local`), `1` = WAN / global / public DNS.
fn classify_web_ui_url_tier(url: &str) -> u8 {
    let n = normalize_base(url);
    let Ok(u) = Url::parse(&n) else {
        return 1;
    };
    let Some(host) = u.host() else {
        return 1;
    };
    match host {
        url::Host::Ipv4(ip) => {
            if ip.is_loopback() || ip.is_unspecified() {
                return 1;
            }
            if ip.is_private() || ip.is_link_local() {
                0
            } else {
                1
            }
        }
        url::Host::Ipv6(ip) => {
            if ip.is_loopback() || ip.is_unspecified() {
                return 1;
            }
            if ip.is_unique_local() || ip.is_unicast_link_local() {
                0
            } else {
                1
            }
        }
        url::Host::Domain(d) => {
            if d.eq_ignore_ascii_case("localhost") {
                return 1;
            }
            if d.to_ascii_lowercase().ends_with(".local") {
                0
            } else {
                1
            }
        }
    }
}

fn merge_and_sort_web_ui_urls(v: &serde_json::Value, active_base: Option<&str>) -> Vec<String> {
    let mut by_key: HashMap<String, String> = HashMap::new();

    if let Some(arr) = v.get("lanWebUrls").and_then(|x| x.as_array()) {
        for x in arr {
            if let Some(s) = x.as_str() {
                let n = normalize_base(s);
                if let Some(k) = web_origin_key(&n) {
                    by_key.entry(k).or_insert(n);
                }
            }
        }
    }

    if let Some(ab) = active_base {
        let n = normalize_base(ab);
        if !http_origin_is_localhost(&n) {
            if let Some(k) = web_origin_key(&n) {
                if !by_key.contains_key(&k) {
                    by_key.insert(k, n);
                }
            }
        }
    }

    let mut urls: Vec<String> = by_key.into_values().collect();
    urls.sort_by(|a, b| {
        classify_web_ui_url_tier(a)
            .cmp(&classify_web_ui_url_tier(b))
            .then_with(|| a.cmp(b))
    });
    if urls.is_empty()
        && let Some(ab) = active_base
    {
        urls.push(normalize_base(ab));
    }
    urls
}

/// `active_base`: normalized `http(s)://host:port` used to reach `/ping` (e.g. configured `server_url`).
fn print_web_ui_urls_hint(v: &serde_json::Value, active_base: Option<&str>) {
    let urls = merge_and_sort_web_ui_urls(v, active_base);
    if urls.is_empty() {
        return;
    }
    println!("You can try these URLs to open the Web UI:");
    for u in urls {
        println!("  {u}");
    }
}

fn print_web_ui_urls_hint_blocking(base: &str) {
    if let Ok(v) = fetch_ping_json_blocking(base) {
        print_web_ui_urls_hint(&v, Some(base));
    }
}

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
    about = "Guan Dan — Server + API client",
    before_help = "Run a GuanDan game server and operate tables, players, and bots from the command line.",
    long_about = "CLI for running a Guan Dan server and operating tables/players/bots via API.\nUse it to start a local server, create or join tables, play actions, and automate games with bots.",
    after_help = "Quick start:\n  clawguandan server start\n  clawguandan table create my-table\n  clawguandan table join -t <table_id> --name Alice\n\nTroubleshooting:\n  1) Run `clawguandan <command> -h` for required flags.\n  2) Runtime failures print `hint:` lines with next steps.\n  3) Most runtime errors exit with code 1."
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
    /// Manage tables: list/create/join/snapshot/sync
    Table {
        #[command(subcommand)]
        cmd: TableCmd,
    },
    /// Play / seat actions
    Play {
        #[command(subcommand)]
        cmd: PlayCmd,
    },
    /// Automate full-table play with built-in, rule-based, or LLM bots
    Bot {
        #[command(subcommand)]
        cmd: BotCmd,
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
    /// Print detailed version/build information
    #[command(name = "version", visible_alias = "verion")]
    Version {
        /// Output as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum BotCmd {
    /// Run simple bots with an always-beat preference strategy. Optionally target an existing table; otherwise create one.
    BeatIt {
        /// Optional existing table ID to join. If omitted, creates a fresh table.
        #[arg(short = 't', long)]
        table: Option<String>,
        /// Starting rank/level for table creation (2-10, J, Q, K, A). Only valid when creating a new table.
        #[arg(long)]
        rank: Option<String>,
        /// Number of bots to join. If omitted, auto-fills all current vacancies.
        #[arg(long)]
        players: Option<u8>,
        /// Number of hands to complete (each ends in scoring). If omitted, runs until game end.
        #[arg(long)]
        hands: Option<u32>,
        /// Increase log verbosity (`-v` summary, `-vv` subprocess stdout, `-vvv` +stderr/raw transition)
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbosity: u8,
    },
    /// Run rule-based bots via subprocess runtime. Optionally target an existing table; otherwise create one.
    RuleBot {
        /// Optional existing table ID to join. If omitted, creates a fresh table.
        #[arg(short = 't', long)]
        table: Option<String>,
        /// Starting rank/level for table creation (2-10, J, Q, K, A). Only valid when creating a new table.
        #[arg(long)]
        rank: Option<String>,
        /// Number of bots to join. If omitted, auto-fills all current vacancies.
        #[arg(long)]
        players: Option<u8>,
        /// Number of hands to complete (each ends in scoring). If omitted, runs until game end.
        #[arg(long)]
        hands: Option<u32>,
        /// Increase log verbosity (`-v` summary, `-vv` subprocess stdout, `-vvv` +stderr/raw transition)
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbosity: u8,
    },
    /// Run LLM-driven bots: each decision invokes your script (stdin prompt, stdout markers).
    LlmBot {
        /// Optional existing table ID to join. If omitted, creates a fresh table.
        #[arg(short = 't', long)]
        table: Option<String>,
        /// Starting rank/level for table creation (2-10, J, Q, K, A). Only valid when creating a new table.
        #[arg(long)]
        rank: Option<String>,
        /// Number of bots to join. If omitted, auto-fills all current vacancies.
        #[arg(long)]
        players: Option<u8>,
        /// Number of hands to complete (each ends in scoring). If omitted, runs until game end.
        #[arg(long)]
        hands: Option<u32>,
        /// Increase log verbosity (`-v` summary + full script stdout, `-vv` + stdin prompt/script stderr, `-vvv` +raw transition)
        #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
        verbosity: u8,
        /// Path to user script: read UTF-8 prompt from stdin, write markers to stdout.
        #[arg(long, conflicts_with = "default_script")]
        script: Option<PathBuf>,
        /// Use built-in default script under temp_dir()/clawguandan/scripts/.
        #[arg(long, value_enum, conflicts_with = "script")]
        default_script: Option<DefaultScriptKind>,
        /// Wall-clock timeout per script invocation in milliseconds (default: 120000).
        #[arg(long)]
        llm_timeout_ms: Option<u64>,
        /// Before join, call script once to parse `<<<NAMING:LIST|...>>>` for bot display names (default: on; pass `false` to disable).
        #[arg(long, action = ArgAction::Set, default_value_t = true, value_name = "true|false")]
        llm_name_bots: bool,
        /// LLM model name for bot joins (`table join --type bot --model ...`).
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum DefaultScriptKind {
    Openclaw,
    Hermes,
}

#[derive(Subcommand)]
#[command(after_help = "Tip: `server` defaults to `status` when no subcommand is given.")]
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
#[command(
    after_help = "Agent tips:\n  - Use `table join -t <id> --name <name>` to get player credentials.\n  - Use `table sync` to refresh local session state before play actions.\n  - Observer mode: omit `-p` and optionally set `--observer-name`."
)]
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
        /// LLM model name. Effective only when `--type bot` (robot player).
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
        /// Observer session subdirectory `observer.<name>` (default `default` when omitted; mutually exclusive with `-p`)
        #[arg(long, conflicts_with = "player_id")]
        observer_name: Option<String>,
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
    /// Omit `-p` for observer mode (session under `.../<hostPortKey>/<tableId>/observer.<name>/`).
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
        /// Observer session subdirectory `observer.<name>` (default `default` when omitted; mutually exclusive with `-p`)
        #[arg(long, conflicts_with = "player_id")]
        observer_name: Option<String>,
    },
}

#[derive(Subcommand)]
#[command(
    after_help = "Agent tips:\n  - Most actions auto-use stored seq; run `table sync` if you see stale-seq errors.\n  - Use `-k/--player-key` to override local credentials when required."
)]
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
        /// Overall maximum wait budget in milliseconds. Omit to wait indefinitely.
        #[arg(long)]
        timeout_ms: Option<u64>,
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
}

fn err_with_hint(message: impl Into<String>, hint: impl AsRef<str>) -> String {
    format!("{}\nhint: {}", message.into(), hint.as_ref())
}

fn err_with_hints(message: impl Into<String>, hints: &[&str]) -> String {
    let mut out = message.into();
    for hint in hints {
        out.push_str("\nhint: ");
        out.push_str(hint);
    }
    out
}

fn err_http_status(action: &str, status: StatusCode, hint: impl AsRef<str>) -> String {
    err_with_hint(
        format!("{action} failed: {status}"),
        format!("{} (status={status})", hint.as_ref()),
    )
}

/// Everything except [`ServerCmd::Serve`], [`ServerCmd::Start`], and [`ServerCmd::Restart`]
/// (those use Tokio in `main`).
pub fn run_from_top(command: Top) -> Result<(), String> {
    match command {
        Top::Server { cmd } => {
            let cmd = cmd.unwrap_or(ServerCmd::Status);
            match cmd {
                ServerCmd::Serve { .. } | ServerCmd::Start { .. } | ServerCmd::Restart { .. } => {
                    Err(err_with_hint(
                        "internal: Serve/Start/Restart must be started from main with a Tokio runtime",
                        "invoke these commands from CLI entrypoint instead of run_from_top()",
                    ))
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
                observer_name,
            } => table_nextstate(
                table_id,
                player_id,
                player_key,
                seq,
                timeout_ms,
                observer_name,
            ),
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
                observer_name,
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
                observer_name,
            ),
        },
        Top::Bot { cmd } => match cmd {
            BotCmd::BeatIt {
                table,
                rank,
                players,
                hands,
                verbosity,
            } => simulate_cliplay_subprocess(table, rank, players, hands, verbosity),
            BotCmd::RuleBot {
                table,
                rank,
                players,
                hands,
                verbosity,
            } => simulate_rule_bot_subprocess(table, rank, players, hands, verbosity),
            BotCmd::LlmBot {
                table,
                rank,
                players,
                hands,
                verbosity,
                script,
                default_script,
                llm_timeout_ms,
                llm_name_bots,
                model,
            } => simulate_llm_subprocess(
                table,
                rank,
                players,
                hands,
                verbosity,
                script,
                default_script,
                llm_timeout_ms,
                llm_name_bots,
                model,
            ),
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
            ShowCmd::Version { json } => {
                let name = env!("CARGO_PKG_NAME");
                let version = env!("CARGO_PKG_VERSION");
                let same = format!("{name} {version}");
                let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
                let build_profile = option_env!("PROFILE");
                if json {
                    let mut obj = serde_json::Map::new();
                    obj.insert("name".to_string(), json!(name));
                    obj.insert("version".to_string(), json!(version));
                    obj.insert("sameAsVersionFlag".to_string(), json!(same));
                    obj.insert("target".to_string(), json!(target));
                    if let Some(profile) = build_profile {
                        obj.insert("buildProfile".to_string(), json!(profile));
                    }
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&Value::Object(obj))
                            .map_err(|e| e.to_string())?
                    );
                } else {
                    println!("name: {name}");
                    println!("version: {version}");
                    println!("same_as_--version: {same}");
                    if let Some(profile) = build_profile {
                        println!("build_profile: {profile}");
                    }
                    println!("target: {target}");
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
        return Err(err_with_hint(
            format!("probe failed: HTTP {}", r.status()),
            "verify server address with `clawguandan server use <host:port>` or start local with `clawguandan server start`",
        ));
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    if v.get("pong").and_then(|x| x.as_str()) != Some("clawguandan") {
        return Err(err_with_hint(
            "probe failed: not a clawguandan server (missing or wrong pong)",
            "target must provide GET /ping and return {\"pong\":\"clawguandan\"}",
        ));
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
        print_web_ui_urls_hint_blocking(&local_base);
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
            err_with_hint(
                format!(
                    "failed to spawn `{}` (set CLAW_GUANDAN_SERVER_BIN): {}",
                    server_bin, e
                ),
                "ensure the binary path exists and is executable, or unset CLAW_GUANDAN_SERVER_BIN",
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
            Ok(()) => {
                if !auto_use {
                    print_web_ui_urls_hint_blocking(&local_base);
                }
                return Ok(());
            }
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

/// Stop whatever serves [`LOCAL_SERVER_PROBE_ADDR`]: GET `/ping` there for PID (ignores config).
/// Success means `kill(pid, 0)` returns `ESRCH`.
pub fn server_stop() -> Result<(), String> {
    let base = normalize_base(LOCAL_SERVER_PROBE_ADDR);
    let pid = ping_pid_blocking(&base).map_err(|e| {
        format!("cannot stop: no clawguandan server on {LOCAL_SERVER_PROBE_ADDR} ({e})")
    })?;

    platform_process::signal_terminate(pid).map_err(|e| format!("terminate: {e}"))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if platform_process::is_process_exited(pid) {
            println!("stopped server (pid {pid})");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    platform_process::signal_force_kill(pid).map_err(|e| format!("force kill: {e}"))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if platform_process::is_process_exited(pid) {
            println!("stopped server (pid {pid}) after force kill");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err(format!(
        "server pid {pid} did not exit; check permissions or process state"
    ))
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
    cfg.server_url = Some(base.clone());
    cfg.save()?;
    println!("active server: {}", cfg.server_url.as_deref().unwrap_or(""));
    print_web_ui_urls_hint_blocking(&base);
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
        Some(url) => match fetch_ping_json_blocking(url) {
            Ok(v) => {
                print_web_ui_urls_hint(&v, Some(url.as_str()));
                "active"
            }
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
        return Err(err_http_status(
            "list",
            r.status(),
            "run `clawguandan server status` and verify the active server before retry",
        ));
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
        return Err(err_http_status(
            "create",
            r.status(),
            "check `--rank` value (2-10,J,Q,K,A) and ensure server is healthy",
        ));
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
        Some("bot") => Some("bot".into()),
        Some("unknown") => Some("unknown".into()),
        Some(x) => {
            return Err(err_with_hint(
                format!("invalid player type {:?}", x),
                "use one of: human, random, smart, bot",
            ));
        }
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
            let enc =
                serde_json::to_string(&Value::String(s.clone())).map_err(|e| e.to_string())?;
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
                let key_json = serde_json::to_string(&Value::String((*k).to_string()))
                    .map_err(|e| e.to_string())?;
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

fn print_materialized_session(
    session: &PlayerSession,
    mode: MaterializedPrintMode,
) -> Result<(), String> {
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
        let pkey =
            player_key.ok_or_else(|| "playerKey is required when playerId is set".to_string())?;
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
        let snap =
            http_get_snapshot_parsed(base, client, table_id, Some(player_id), Some(player_key))?;
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
    observer_name: &str,
) -> Result<PlayerSession, String> {
    let mut s = read_observer_session(base, table_id, observer_name)?.unwrap_or_default();
    if s.table_state.is_none() {
        let snap = http_get_snapshot_parsed(base, client, table_id, None, None)?;
        s.table_state = Some(snap.state);
        s.private_view = None;
        s.last_applied_seq = s.table_state.as_ref().map(|t| t.seq).unwrap_or(0);
        write_observer_session(base, table_id, observer_name, &s)?;
    }
    Ok(s)
}

fn merge_nextstate_into_observer_session(
    base: &str,
    client: &Client,
    table_id: &str,
    observer_name: &str,
    body: &NextStateBody,
) -> Result<(), String> {
    let mut s = ensure_session_bootstrap_observer(base, client, table_id, observer_name)?;
    let ts = s
        .table_state
        .as_mut()
        .ok_or_else(|| "bootstrap left table_state empty".to_string())?;
    let new_ts = apply_transition_delta_to_table_state(ts, &body.transition.delta)
        .map_err(|e| format!("apply transition delta: {e}"))?;
    *ts = new_ts;
    s.last_applied_seq = body.transition.seq;
    s.private_view = None;
    write_observer_session(base, table_id, observer_name, &s)?;
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
        read_session_last_applied_seq(&base, &table_id, &player_id)?.ok_or_else(|| {
            err_with_hints(
                "auto-seq: no stored lastAppliedSeq for this player",
                &[
                    "run `table sync -t <table_id> -p <player_id>` to bootstrap local session",
                    "or pass `--seq` explicitly",
                ],
            )
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
        return Err(err_http_status(
            "join",
            r.status(),
            "verify --table-id exists and --seat is one of auto/n/e/s/w",
        ));
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
    table_sync(table_id, Some(pid), Some(pkey), None, None, None)
}

fn table_nextstate(
    table_id: String,
    player_id: Option<String>,
    player_key: Option<String>,
    manual_seq: Option<u64>,
    timeout_ms: u64,
    observer_name: Option<String>,
) -> Result<(), String> {
    let base = load_active_server_base()?;
    let client = http_client()?;

    let observer_key: Option<&str> = if player_id.is_none() && manual_seq.is_none() {
        let on = observer_name.as_deref().unwrap_or("default").trim();
        if on.is_empty() {
            return Err(err_with_hint(
                "invalid --observer-name: empty",
                "omit --observer-name to use default, or provide a non-empty value",
            ));
        }
        validate_session_id_component(on, "observer_name")?;
        Some(on)
    } else {
        None
    };

    let since_seq = if let Some(s) = manual_seq {
        s
    } else if let Some(ref pid) = player_id {
        read_session_last_applied_seq(&base, &table_id, pid)?.unwrap_or(0)
    } else {
        let on = observer_key.ok_or_else(|| {
            "internal: observer auto-seq requires table nextstate without --seq and without -p"
                .to_string()
        })?;
        read_session_last_applied_seq_observer(&base, &table_id, on)?.unwrap_or(0)
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
                let on = observer_key.ok_or_else(|| {
                    "internal: observer session merge requires table nextstate without --seq and without -p"
                        .to_string()
                })?;
                merge_nextstate_into_observer_session(&base, &client, &table_id, on, &body)?;
            }
            Ok(())
        }
        _ => Err(err_http_status(
            "nextstate",
            r.status(),
            "verify table id and credentials, then retry or run `table snapshot` for diagnostics",
        )),
    }
}

fn table_sync(
    table_id: String,
    player_id: Option<String>,
    player_key: Option<String>,
    manual_seq: Option<u64>,
    print: Option<MaterializedPrintMode>,
    observer_name: Option<String>,
) -> Result<(), String> {
    if manual_seq.is_some() {
        return Err(err_with_hint(
            "table sync does not support --seq (uses session auto-seq)",
            "remove --seq; for manual seq control use `table nextstate --seq`",
        ));
    }
    let base = load_active_server_base()?;
    let client = http_client()?;

    // Each request uses timeoutMs=0: server returns 204 immediately when already at head.
    const NEXTSTATE_TIMEOUT_MS: u64 = 0;

    match &player_id {
        None => {
            let on = observer_name.as_deref().unwrap_or("default").trim();
            if on.is_empty() {
                return Err(err_with_hint(
                    "invalid --observer-name: empty",
                    "omit --observer-name to use default, or provide a non-empty value",
                ));
            }
            validate_session_id_component(on, "observer_name")?;
            ensure_session_bootstrap_observer(&base, &client, &table_id, on)?;
            loop {
                let since_seq =
                    read_session_last_applied_seq_observer(&base, &table_id, on)?.unwrap_or(0);
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
                        merge_nextstate_into_observer_session(
                            &base, &client, &table_id, on, &body,
                        )?;
                        if body.lag == 0 {
                            break;
                        }
                    }
                    _ => {
                        return Err(err_http_status(
                            "nextstate",
                            r.status(),
                            "verify table id and active server before retrying sync",
                        ));
                    }
                }
            }

            let s = read_observer_session(&base, &table_id, on)?
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
                    _ => {
                        return Err(err_http_status(
                            "nextstate",
                            r.status(),
                            "verify -p/-k credentials and table state before retrying sync",
                        ));
                    }
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
    timeout_ms: Option<u64>,
    print_mode: MaterializedPrintMode,
) -> Result<(), String> {
    const WAIT4MYTURN_NEXTSTATE_MAX_TIMEOUT_MS: u64 = 60_000;
    if manual_seq.is_some() {
        return Err(err_with_hint(
            "play wait4myturn does not support --seq (uses session auto-seq)",
            "remove --seq and run `table sync -t <table_id> -p <player_id>` first if session is stale",
        ));
    }
    // Catch up to server head with timeoutMs=0 nextstate loop (no long-poll at head), so the
    // local shortcut below cannot fire on a stale session while the table has moved on.
    table_sync(
        table_id.clone(),
        Some(player_id.clone()),
        player_key.clone(),
        None,
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

    let budget_ms = timeout_ms;
    let deadline = budget_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
    loop {
        let poll_timeout_ms: u64 = match deadline {
            Some(dl) => {
                let now = Instant::now();
                if now >= dl {
                    return Err(err_with_hint(
                        format!(
                            "wait4myturn timeout after {}ms",
                            budget_ms.unwrap_or_default()
                        ),
                        "increase --timeout-ms, or inspect state via `table snapshot` / `table nextstate`",
                    ));
                }
                let remaining_ms_u128 = dl.saturating_duration_since(now).as_millis();
                let remaining_ms = u64::try_from(remaining_ms_u128).unwrap_or(u64::MAX);
                remaining_ms
                    .min(WAIT4MYTURN_NEXTSTATE_MAX_TIMEOUT_MS)
                    .max(1)
            }
            None => WAIT4MYTURN_NEXTSTATE_MAX_TIMEOUT_MS,
        };
        let since_seq = read_session_last_applied_seq(&base, &table_id, &player_id)?.unwrap_or(0);
        let mut u = url::Url::parse(&format!("{}/api/v1/tables/{}/nextstate", base, table_id))
            .map_err(|e| e.to_string())?;
        u.query_pairs_mut()
            .append_pair("sinceSeq", &since_seq.to_string())
            .append_pair("timeoutMs", &poll_timeout_ms.to_string());
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
            _ => {
                return Err(err_http_status(
                    "nextstate",
                    r.status(),
                    "verify table id and player credentials, then retry `play wait4myturn`",
                ));
            }
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
    table_sync(table_id, Some(player_id), player_key, None, None, None)
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
            read_session_last_applied_seq(&base, &table_id, &player_id)?.ok_or_else(|| {
                err_with_hints(
                    "auto-seq: no stored lastAppliedSeq for this player",
                    &[
                        "run `table sync -t <table_id> -p <player_id>` to refresh local session",
                        "or pass `--seq` explicitly",
                    ],
                )
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
                None,
            )?;
            retried_after_stale_seq = true;
            continue;
        }
        return Err(err_with_hints(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| format!("{action} failed")),
            &[
                "if error code is STALE_SEQ, run `table sync` then retry",
                "use `-k/--player-key` to override stale local credentials when needed",
            ],
        ));
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn parse_cli_stdout_json(out: &[u8]) -> Result<serde_json::Value, String> {
    let s = String::from_utf8_lossy(out);
    let t = s.trim();
    serde_json::from_str(t).map_err(|e| format!("invalid JSON from CLI: {e}; got: {t:?}"))
}

#[cfg(test)]
#[allow(dead_code)]
fn nextstate_stdout_is_no_content(stdout: &[u8]) -> bool {
    let t = String::from_utf8_lossy(stdout).trim().to_string();
    t.is_empty() || t.starts_with("(no new transition within timeout)")
}

#[cfg(test)]
fn transition_counts_as_hand_done(v: &serde_json::Value) -> bool {
    matches!(
        v.get("type").and_then(|x| x.as_str()),
        Some("HAND_ENDED_WAITING_READY" | "GAME_COMPLETED")
    )
}

#[cfg(test)]
#[allow(dead_code)]
fn table_state_is_terminal(v: &serde_json::Value) -> bool {
    v.get("status").and_then(|x| x.as_str()) == Some("finished")
        || v.get("expect")
            .and_then(|e| e.get("kind"))
            .and_then(|x| x.as_str())
            == Some("game_over")
}

#[cfg(test)]
#[allow(dead_code)]
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

#[cfg(test)]
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

#[cfg(test)]
#[allow(dead_code)]
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
    fn merge_web_urls_lan_before_wan_and_adds_active_wan() {
        let v = json!({
            "pong": "clawguandan",
            "lanWebUrls": ["http://8.8.8.8:1", "http://192.168.2.1:80"],
        });
        let merged = merge_and_sort_web_ui_urls(&v, Some("http://198.51.100.2:9999"));
        assert_eq!(
            merged,
            vec![
                "http://192.168.2.1:80".to_string(),
                "http://198.51.100.2:9999".to_string(),
                "http://8.8.8.8:1".to_string(),
            ]
        );
    }

    #[test]
    fn merge_web_urls_skips_active_localhost_and_dedups_same_origin() {
        let v = json!({
            "pong": "clawguandan",
            "lanWebUrls": ["http://192.168.1.10:22222"],
        });
        let only_lan = merge_and_sort_web_ui_urls(&v, Some("http://127.0.0.1:22222"));
        assert_eq!(only_lan, vec!["http://192.168.1.10:22222".to_string()]);

        let dedup = merge_and_sort_web_ui_urls(&v, Some("http://192.168.1.10:22222"));
        assert_eq!(dedup, vec!["http://192.168.1.10:22222".to_string()]);
    }

    #[test]
    fn merge_web_urls_falls_back_to_active_when_ping_urls_empty() {
        let v = json!({
            "pong": "clawguandan",
            "lanWebUrls": [],
        });
        let merged = merge_and_sort_web_ui_urls(&v, Some("127.0.0.1:22222"));
        assert_eq!(merged, vec!["http://127.0.0.1:22222".to_string()]);
    }

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

#[cfg(test)]
#[allow(dead_code)]
struct CliplayShared {
    stop: AtomicBool,
    start_seq: u64,
    last_scoring_transition_seq: Mutex<Option<u64>>,
    hands_done: AtomicU32,
    hands_target: u32,
    err: Mutex<Option<String>>,
}

#[cfg(test)]
#[allow(dead_code)]
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
        println!("\n--- bot beat-it: hand {n} completed (transition seq={tr_seq}) ---");
        if n >= self.hands_target {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn on_transition_maybe_terminal(&self, v: &serde_json::Value) {
        let terminal_by_type = v.get("type").and_then(|x| x.as_str()) == Some("GAME_COMPLETED");
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
    hands: Option<u32>,
    verbosity: u8,
) -> Result<(), String> {
    run_bot_subprocess(
        BotRunOptions {
            table,
            rank,
            players,
            hands,
            verbosity,
        },
        Arc::new(BeatItPlugin),
    )
}

fn simulate_rule_bot_subprocess(
    table: Option<String>,
    rank: Option<String>,
    players: Option<u8>,
    hands: Option<u32>,
    verbosity: u8,
) -> Result<(), String> {
    run_bot_subprocess(
        BotRunOptions {
            table,
            rank,
            players,
            hands,
            verbosity,
        },
        Arc::new(RuleBotPlugin::default()),
    )
}

fn simulate_llm_subprocess(
    table: Option<String>,
    rank: Option<String>,
    players: Option<u8>,
    hands: Option<u32>,
    verbosity: u8,
    script: Option<PathBuf>,
    default_script: Option<DefaultScriptKind>,
    llm_timeout_ms: Option<u64>,
    llm_name_bots: bool,
    model: Option<String>,
) -> Result<(), String> {
    let timeout_ms = llm_timeout_ms.unwrap_or(120_000);
    let resolved = resolve_llm_script(script, default_script)?;
    println!("[llm-bot] script:init {}", resolved.init_message);
    let detected_model = verify_llm_script_model(
        &resolved.path,
        Duration::from_millis(timeout_ms),
        resolved.is_default_script,
    )?;
    let resolved_model = resolve_join_model(model, detected_model);
    if let Some(ref m) = resolved_model {
        println!("[llm-bot] script:model {}", m);
    }
    run_bot_subprocess(
        BotRunOptions {
            table,
            rank,
            players,
            hands,
            verbosity,
        },
        Arc::new(LlmBotPlugin::new(LlmBotParams {
            script: resolved.path,
            timeout: Duration::from_millis(timeout_ms),
            name_bots: llm_name_bots,
            model: resolved_model,
            verbosity,
        })),
    )
}

struct ResolvedLlmScript {
    path: PathBuf,
    is_default_script: bool,
    init_message: String,
}

fn resolve_llm_script(
    script: Option<PathBuf>,
    default_script: Option<DefaultScriptKind>,
) -> Result<ResolvedLlmScript, String> {
    match (script, default_script) {
        (Some(path), None) => Ok(ResolvedLlmScript {
            init_message: format!("load custom script={}", path.display()),
            path,
            is_default_script: false,
        }),
        (None, Some(kind)) => {
            let (path, created) = ensure_default_script(kind)?;
            let action = if created { "create" } else { "load" };
            Ok(ResolvedLlmScript {
                init_message: format!("{action} default script={}", path.display()),
                path,
                is_default_script: true,
            })
        }
        (None, None) => Err("must provide either --script or --default-script".into()),
        (Some(_), Some(_)) => Err("--script and --default-script are mutually exclusive".into()),
    }
}

fn default_script_path(kind: DefaultScriptKind) -> PathBuf {
    let file = match kind {
        DefaultScriptKind::Openclaw => "ask_openclaw.sh",
        DefaultScriptKind::Hermes => "ask_hermes.sh",
    };
    session_state_root().join("scripts").join(file)
}

fn default_script_body(kind: DefaultScriptKind) -> &'static str {
    match kind {
        DefaultScriptKind::Openclaw => {
            r#"#!/usr/bin/env bash

PROMPT=$(cat)
openclaw agent --message "$PROMPT" --local --session-id "ask_openclaw_$(date +%s)"
"#
        }
        DefaultScriptKind::Hermes => {
            r#"#!/usr/bin/env bash

PROMPT=$(cat)
hermes chat -q "$PROMPT" --quiet
"#
        }
    }
}

fn ensure_default_script(kind: DefaultScriptKind) -> Result<(PathBuf, bool), String> {
    let path = default_script_path(kind);
    if path.exists() {
        ensure_executable(&path)?;
        return Ok((path, false));
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("default script has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("create default script dir {}: {e}", parent.display()))?;
    fs::write(&path, default_script_body(kind))
        .map_err(|e| format!("write default script {}: {e}", path.display()))?;
    ensure_executable(&path)?;
    Ok((path, true))
}

fn ensure_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)
            .map_err(|e| format!("stat script {}: {e}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("chmod +x {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Self-check: stdin prompt asks for `<<<MODEL:name>>>`, used for both `--script` and `--default-script`.
fn verify_llm_script_model(
    script: &Path,
    timeout: Duration,
    is_default_script: bool,
) -> Result<String, String> {
    match verify_script_model(script, timeout) {
        Ok(model) => Ok(model),
        Err(e) => {
            eprintln!("[llm-bot] script self-check error: {e}");
            Err(llm_script_self_check_failure_message(
                is_default_script,
                script,
            ))
        }
    }
}

fn llm_script_self_check_failure_message(is_default_script: bool, script: &Path) -> String {
    if is_default_script {
        format!(
            "The default script is not working, please fix it and rerun or use another script via --script. The default script file is at {}",
            script.display()
        )
    } else {
        format!(
            "The script passed via --script is not working; fix the script or choose another path. Script path: {}",
            script.display()
        )
    }
}

#[cfg(test)]
#[test]
fn session_dirs_use_host_table_leaf_layout() {
    let base = "http://127.0.0.1:22222";
    let pd = player_session_dir(base, "t_abc", "p_xyz").unwrap();
    assert_eq!(pd.file_name().and_then(|s| s.to_str()), Some("p_xyz"));
    assert_eq!(
        pd.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str()),
        Some("t_abc")
    );
    assert_eq!(
        pd.parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str()),
        Some("127.0.0.1_22222")
    );
    assert!(pd.starts_with(session_state_root()));

    let od = observer_session_dir(base, "t_abc", "default").unwrap();
    assert_eq!(
        od.file_name().and_then(|s| s.to_str()),
        Some("observer.default")
    );
    assert_eq!(
        od.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str()),
        Some("t_abc")
    );
    assert!(od.starts_with(session_state_root()));
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
