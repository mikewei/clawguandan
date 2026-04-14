//! Cross-process CLI helpers: run the `clawguandan` binary for integration tests and automation.

use std::path::Path;
use std::process::{Command, Output};

#[derive(Debug)]
pub enum CliRunError {
    Io(std::io::Error),
    NonZeroStatus { code: Option<i32>, stderr: String },
}

impl std::fmt::Display for CliRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliRunError::Io(e) => write!(f, "{e}"),
            CliRunError::NonZeroStatus { code, stderr } => {
                write!(f, "cli exited with {:?}: {}", code, stderr)
            }
        }
    }
}

impl std::error::Error for CliRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliRunError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CliRunError {
    fn from(e: std::io::Error) -> Self {
        CliRunError::Io(e)
    }
}

/// Run `clawguandan` (or another binary) with the given args; on non-zero status, return [`CliRunError`].
pub fn run_cli_command(
    bin: &Path,
    args: &[impl AsRef<std::ffi::OsStr>],
) -> Result<Output, CliRunError> {
    let out = Command::new(bin).args(args).output()?;
    if !out.status.success() {
        return Err(CliRunError::NonZeroStatus {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(out)
}

/// `clawguandan table create` argv (omit binary name; pass as `run_cli_command(bin, &args)`).
pub fn cli_argv_table_create(name: Option<&str>, rank: Option<&str>) -> Vec<String> {
    let mut v = vec!["table".into(), "create".into()];
    if let Some(n) = name {
        v.push(n.to_string());
    }
    if let Some(r) = rank {
        v.push("--rank".into());
        v.push(r.to_string());
    }
    v
}

/// `clawguandan table join ...`
pub fn cli_argv_table_join(table_id: &str, player_name: &str, seat: &str) -> Vec<String> {
    vec![
        "table".into(),
        "join".into(),
        "-t".into(),
        table_id.to_string(),
        "--name".into(),
        player_name.to_string(),
        "--seat".into(),
        seat.to_string(),
    ]
}

/// `clawguandan play ready ...` (requires configured server URL for a real run).
pub fn cli_argv_play_ready(table_id: &str, player_id: &str) -> Vec<String> {
    vec![
        "play".into(),
        "ready".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
    ]
}

/// `clawguandan play wait4myturn -t ... -p ... --timeout-ms ...` (session auto-seq only).
pub fn cli_argv_play_wait4myturn(table_id: &str, player_id: &str, timeout_ms: u64) -> Vec<String> {
    vec![
        "play".into(),
        "wait4myturn".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
        "--timeout-ms".into(),
        timeout_ms.to_string(),
    ]
}

/// `clawguandan play pass ...`
pub fn cli_argv_play_pass(table_id: &str, player_id: &str, seq: u64) -> Vec<String> {
    vec![
        "play".into(),
        "pass".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
        "--seq".into(),
        seq.to_string(),
    ]
}

/// `clawguandan play playcards ...` — `cards` is comma-separated symbols (see CLI).
pub fn cli_argv_play_playcards(
    table_id: &str,
    player_id: &str,
    seq: u64,
    cards_csv: &str,
) -> Vec<String> {
    vec![
        "play".into(),
        "playcards".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
        "--seq".into(),
        seq.to_string(),
        cards_csv.to_string(),
    ]
}

/// `clawguandan play playcards ... --wild-targets ...`
pub fn cli_argv_play_playcards_wild(
    table_id: &str,
    player_id: &str,
    seq: u64,
    cards_csv: &str,
    wild_targets_csv: &str,
) -> Vec<String> {
    let mut v = cli_argv_play_playcards(table_id, player_id, seq, cards_csv);
    v.push("--wild-targets".into());
    v.push(wild_targets_csv.to_string());
    v
}

/// `clawguandan table nextstate -t ... -p ... --seq ... --timeout-ms ...`
pub fn cli_argv_table_nextstate(
    table_id: &str,
    player_id: &str,
    since_seq: u64,
    timeout_ms: u64,
) -> Vec<String> {
    vec![
        "table".into(),
        "nextstate".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
        "--seq".into(),
        since_seq.to_string(),
        "--timeout-ms".into(),
        timeout_ms.to_string(),
    ]
}

/// `clawguandan play suggest -t ... -p ...` with optional `--seq` (omit for auto-seq).
pub fn cli_argv_play_suggest(table_id: &str, player_id: &str, seq: Option<u64>) -> Vec<String> {
    let mut v = vec![
        "play".into(),
        "suggest".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
    ];
    if let Some(s) = seq {
        v.push("--seq".into());
        v.push(s.to_string());
    }
    v
}

/// Observer `clawguandan table nextstate -t ... --timeout-ms ...` (no `-p`, session auto-seq).
pub fn cli_argv_table_nextstate_observer(table_id: &str, timeout_ms: u64) -> Vec<String> {
    vec![
        "table".into(),
        "nextstate".into(),
        "-t".into(),
        table_id.to_string(),
        "--timeout-ms".into(),
        timeout_ms.to_string(),
    ]
}

/// `clawguandan table snapshot -t ...` with optional `-p`
pub fn cli_argv_table_snapshot(table_id: &str, player_id: Option<&str>) -> Vec<String> {
    let mut v = vec![
        "table".into(),
        "snapshot".into(),
        "-t".into(),
        table_id.to_string(),
    ];
    if let Some(pid) = player_id {
        v.push("-p".into());
        v.push(pid.to_string());
    }
    v
}

/// `clawguandan play tribute ...`
pub fn cli_argv_play_tribute(table_id: &str, player_id: &str, seq: u64, card: &str) -> Vec<String> {
    vec![
        "play".into(),
        "tribute".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
        "--seq".into(),
        seq.to_string(),
        card.to_string(),
    ]
}

/// `clawguandan play returncard ...`
pub fn cli_argv_play_returncard(
    table_id: &str,
    player_id: &str,
    seq: u64,
    card: &str,
) -> Vec<String> {
    vec![
        "play".into(),
        "returncard".into(),
        "-t".into(),
        table_id.to_string(),
        "-p".into(),
        player_id.to_string(),
        "--seq".into(),
        seq.to_string(),
        card.to_string(),
    ]
}
