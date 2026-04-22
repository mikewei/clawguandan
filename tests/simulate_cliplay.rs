//! `bot beat-it` argv helpers (fast) and optional full subprocess E2E (ignored by default).

use clawguandan::simulation::{
    cli_argv_play_suggest, cli_argv_play_wait4myturn, cli_argv_table_nextstate,
    cli_argv_table_nextstate_observer, cli_argv_table_snapshot,
};
use serde_json::Value;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_clawguandan"))
}

fn wait_port_open(addr: &str, port: u16, attempts: u32) {
    for _ in 0..attempts {
        if TcpStream::connect((addr, port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("port {port} did not open in time");
}

#[test]
fn cli_argv_play_suggest_matches_cli() {
    assert_eq!(
        cli_argv_play_suggest("t_abc", "p_xyz", Some(42)),
        vec![
            "play".to_string(),
            "suggest".to_string(),
            "-t".to_string(),
            "t_abc".to_string(),
            "-p".to_string(),
            "p_xyz".to_string(),
            "--seq".to_string(),
            "42".to_string(),
        ]
    );
    assert_eq!(
        cli_argv_play_suggest("t_abc", "p_xyz", None),
        vec![
            "play".to_string(),
            "suggest".to_string(),
            "-t".to_string(),
            "t_abc".to_string(),
            "-p".to_string(),
            "p_xyz".to_string(),
        ]
    );
}

#[test]
fn cli_argv_play_wait4myturn_matches_cli() {
    assert_eq!(
        cli_argv_play_wait4myturn("t_abc", "p_xyz", 60_000),
        vec![
            "play".to_string(),
            "wait4myturn".to_string(),
            "-t".to_string(),
            "t_abc".to_string(),
            "-p".to_string(),
            "p_xyz".to_string(),
            "--timeout-ms".to_string(),
            "60000".to_string(),
        ]
    );
}

#[test]
fn cli_argv_table_nextstate_observer_matches_cli() {
    assert_eq!(
        cli_argv_table_nextstate_observer("t_abc", 60_000),
        vec![
            "table".to_string(),
            "nextstate".to_string(),
            "-t".to_string(),
            "t_abc".to_string(),
            "--timeout-ms".to_string(),
            "60000".to_string(),
        ]
    );
}

#[test]
fn cli_argv_table_nextstate_matches_cli() {
    assert_eq!(
        cli_argv_table_nextstate("t_abc", "p_xyz", 8, 60_000),
        vec![
            "table".to_string(),
            "nextstate".to_string(),
            "-t".to_string(),
            "t_abc".to_string(),
            "-p".to_string(),
            "p_xyz".to_string(),
            "--seq".to_string(),
            "8".to_string(),
            "--timeout-ms".to_string(),
            "60000".to_string(),
        ]
    );
}

#[test]
fn cli_argv_table_snapshot_optional_player() {
    assert_eq!(
        cli_argv_table_snapshot("t_abc", None),
        vec![
            "table".to_string(),
            "snapshot".to_string(),
            "-t".to_string(),
            "t_abc".to_string()
        ]
    );
    assert_eq!(
        cli_argv_table_snapshot("t_abc", Some("p1")),
        vec![
            "table".to_string(),
            "snapshot".to_string(),
            "-t".to_string(),
            "t_abc".to_string(),
            "-p".to_string(),
            "p1".to_string(),
        ]
    );
}

#[test]
fn simulate_cliplay_help_includes_table_and_players_flags() {
    let out = Command::new(cargo_bin())
        .args(["bot", "beat-it", "--help"])
        .output()
        .expect("bot beat-it --help");
    assert!(
        out.status.success(),
        "help failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--table"));
    assert!(help.contains("--players"));
}

/// Spawns a real server and runs `clawguandan bot beat-it` (slow; subprocess per step).
#[test]
#[ignore = "manual / CI optional: requires free port and ~minutes for one full hand"]
fn simulate_cliplay_one_hand_exits_zero() {
    use std::io::Read;

    let home = std::env::temp_dir().join(format!("clawguandan_sim_cliplay_{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("temp home");
    let home = home.as_path();

    let port: u16 = 22_400 + (std::process::id() as u16 % 200);
    let addr = "127.0.0.1";

    let mut server = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["server", "serve", "--ip", addr, "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    wait_port_open(addr, port, 100);

    let use_out = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["server", "use", &format!("{addr}:{port}")])
        .output()
        .expect("server use");
    assert!(
        use_out.status.success(),
        "server use failed: {}",
        String::from_utf8_lossy(&use_out.stderr)
    );

    let mut ok = false;
    for _ in 0..80 {
        let ping = Command::new(cargo_bin())
            .env("HOME", home)
            .args(["server", "status"])
            .output();
        if let Ok(o) = ping
            && o.status.success()
            && String::from_utf8_lossy(&o.stdout).contains("active")
        {
            ok = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ok, "server did not become reachable");

    let sim = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["bot", "beat-it", "--players", "4", "--hands", "1"])
        .output()
        .expect("bot beat-it");

    let _ = server.kill();
    let mut stderr = String::new();
    if let Some(mut r) = server.stderr.take() {
        let _ = r.read_to_string(&mut stderr);
    }

    assert!(
        sim.status.success(),
        "bot beat-it failed: stdout={} stderr={} server_stderr={}",
        String::from_utf8_lossy(&sim.stdout),
        String::from_utf8_lossy(&sim.stderr),
        stderr
    );

    let out = String::from_utf8_lossy(&sim.stdout);
    assert!(
        out.contains("bot beat-it done"),
        "expected completion banner, got: {out}"
    );
}

/// Reuse an existing table via `--table` and join only requested bots.
#[test]
#[ignore = "manual / CI optional: requires free port and ~minutes for one full hand"]
fn simulate_cliplay_existing_table_with_players_flag() {
    let home = std::env::temp_dir().join(format!(
        "clawguandan_sim_cliplay_existing_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&home).expect("temp home");
    let home = home.as_path();

    let port: u16 = 22_650 + (std::process::id() as u16 % 150);
    let addr = "127.0.0.1";

    let mut server = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["server", "serve", "--ip", addr, "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn server");

    wait_port_open(addr, port, 100);

    let use_out = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["server", "use", &format!("{addr}:{port}")])
        .output()
        .expect("server use");
    assert!(
        use_out.status.success(),
        "server use failed: {}",
        String::from_utf8_lossy(&use_out.stderr)
    );

    let create = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["table", "create", "simulate-existing"])
        .output()
        .expect("table create");
    assert!(
        create.status.success(),
        "table create failed: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let v: Value = serde_json::from_slice(&create.stdout).expect("parse table create");
    let table_id = v["tableId"].as_str().expect("tableId").to_string();

    let sim = Command::new(cargo_bin())
        .env("HOME", home)
        .args([
            "bot",
            "beat-it",
            "--table",
            &table_id,
            "--players",
            "4",
            "--hands",
            "1",
        ])
        .output()
        .expect("bot beat-it");

    let _ = server.kill();
    assert!(
        sim.status.success(),
        "bot beat-it failed: stdout={} stderr={}",
        String::from_utf8_lossy(&sim.stdout),
        String::from_utf8_lossy(&sim.stderr)
    );
    let out = String::from_utf8_lossy(&sim.stdout);
    assert!(out.contains("bot beat-it done"));
}

#[test]
#[ignore = "manual / CI optional: requires free port and setup for full table"]
fn simulate_cliplay_existing_full_table_without_players_fails() {
    let home = std::env::temp_dir().join(format!(
        "clawguandan_sim_cliplay_full_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&home).expect("temp home");
    let home = home.as_path();

    let port: u16 = 22_820 + (std::process::id() as u16 % 120);
    let addr = "127.0.0.1";

    let mut server = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["server", "serve", "--ip", addr, "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");

    wait_port_open(addr, port, 100);

    let use_out = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["server", "use", &format!("{addr}:{port}")])
        .output()
        .expect("server use");
    assert!(use_out.status.success());

    let create = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["table", "create", "full-table"])
        .output()
        .expect("table create");
    assert!(create.status.success());
    let v: Value = serde_json::from_slice(&create.stdout).expect("parse table create");
    let table_id = v["tableId"].as_str().expect("tableId").to_string();

    for i in 0..4 {
        let join = Command::new(cargo_bin())
            .env("HOME", home)
            .args([
                "table",
                "join",
                "-t",
                &table_id,
                "--name",
                &format!("u{i}"),
                "--seat",
                "auto",
            ])
            .output()
            .expect("table join");
        assert!(
            join.status.success(),
            "join failed at {i}: {}",
            String::from_utf8_lossy(&join.stderr)
        );
    }

    let sim = Command::new(cargo_bin())
        .env("HOME", home)
        .args(["bot", "beat-it", "--table", &table_id, "--hands", "1"])
        .output()
        .expect("bot beat-it");

    let _ = server.kill();
    assert!(
        !sim.status.success(),
        "expected failure, got stdout={} stderr={}",
        String::from_utf8_lossy(&sim.stdout),
        String::from_utf8_lossy(&sim.stderr)
    );
    let stderr = String::from_utf8_lossy(&sim.stderr);
    assert!(
        stderr.contains("no seat vacancy"),
        "expected no seat vacancy error, got: {stderr}"
    );
}
