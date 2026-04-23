use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::bot::plugin::{BotDecision, BotPlugin, BotTurnContext, JoinNamesContext};
use crate::bot::policies::{
    ObserverGameOverContext, ObserverGameStartContext, ObserverHandOverContext,
    ObserverHandStartContext, default_display_names_for_plugin,
};
use crate::domain::TableState;
use crate::game::engine::PlayerAction;
use crate::simulation::run_cli_command;

const NEXTSTATE_TIMEOUT_MS: u64 = 10_000;
const BOT_WAIT4MYTURN_TIMEOUT_MS: u64 = 10_000;
const MAX_STEPS: u64 = 500_000;

#[derive(Clone, Debug)]
pub struct BotRunOptions {
    pub table: Option<String>,
    pub rank: Option<String>,
    pub players: Option<u8>,
    pub hands: Option<u32>,
    pub verbosity: u8,
}

impl BotRunOptions {
    fn show_summary(&self) -> bool {
        self.verbosity >= 1
    }

    fn show_cli_io(&self) -> bool {
        self.verbosity >= 2
    }

    fn show_cli_stderr(&self) -> bool {
        self.verbosity >= 3
    }
}

struct RuntimeShared {
    stop: AtomicBool,
    start_seq: u64,
    last_scoring_transition_seq: Mutex<Option<u64>>,
    hands_done: AtomicU32,
    hands_target: Option<u32>,
    err: Mutex<Option<String>>,
}

impl RuntimeShared {
    fn fail(&self, msg: String) {
        eprintln!("[llm-bot][E][runtime] {msg}");
        let mut e = self.err.lock().unwrap();
        if e.is_none() {
            *e = Some(msg);
        }
        self.stop.store(true, Ordering::SeqCst);
    }

    fn on_transition_maybe_scoring(&self, v: &Value) -> Option<(u32, u64, String)> {
        if !transition_counts_as_hand_done(v) {
            return None;
        }
        let Some(tr_seq) = v.get("seq").and_then(|x| x.as_u64()) else {
            return None;
        };
        if tr_seq <= self.start_seq {
            return None;
        }
        let mut last = self.last_scoring_transition_seq.lock().unwrap();
        if *last == Some(tr_seq) {
            return None;
        }
        *last = Some(tr_seq);
        let n = self.hands_done.fetch_add(1, Ordering::SeqCst) + 1;
        let tr_type = v
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();
        if let Some(hands_target) = self.hands_target
            && n >= hands_target
        {
            self.stop.store(true, Ordering::SeqCst);
        }
        Some((n, tr_seq, tr_type))
    }

    fn on_transition_maybe_terminal(&self, v: &Value) -> bool {
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
            return true;
        }
        false
    }
}

pub fn run_bot_subprocess(
    opts: BotRunOptions,
    plugin: Arc<dyn BotPlugin>,
) -> Result<(), String> {
    if opts.hands == Some(0) {
        return Err("--hands must be >= 1 when provided".into());
    }
    if let Some(n) = opts.players && n > 4 {
        return Err("--players must be <= 4".into());
    }

    let plugin_id = plugin.plugin_id().to_string();
    let bot_label = format!("bot {}", plugin_id);
    let bin = std::env::current_exe().map_err(|e| e.to_string())?;
    let show_cli_io = opts.show_cli_io();
    let show_cli_stderr = opts.show_cli_stderr();
    let observer_policy = plugin.observer_policy();

    let table_id = if let Some(tid) = opts.table.clone() {
        if opts.rank.is_some() {
            return Err("--rank is only allowed when creating a new table (omit --table)".into());
        }
        if show_cli_io {
            println!("[{plugin_id}][D][table:target] table={tid}");
        }
        tid
    } else {
        let label = "table create";
        let mut create_args = vec![
            "table".to_string(),
            "create".to_string(),
            format!("bot-{}", plugin_id),
        ];
        if let Some(rank) = opts.rank.as_deref() {
            create_args.push("--rank".to_string());
            create_args.push(rank.to_string());
        }
        if show_cli_io {
            log_cli_call(&plugin_id, label, &create_args);
        }
        let out = run_cli_command(&bin, &create_args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if show_cli_io {
            log_cli_stdout(&plugin_id, label, &stdout);
        }
        let create_v = parse_cli_stdout_json(&out.stdout)?;
        let tid = create_v["tableId"]
            .as_str()
            .or_else(|| create_v["table_id"].as_str())
            .ok_or_else(|| "create: missing tableId".to_string())?
            .to_string();
        if show_cli_stderr {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.trim().is_empty() {
                log_cli_stderr(&plugin_id, label, &stderr);
            }
        }
        tid
    };

    let snapshot_args = vec![
        "table".to_string(),
        "snapshot".to_string(),
        "-t".to_string(),
        table_id.clone(),
    ];
    if show_cli_io {
        log_cli_call(&plugin_id, "table snapshot", &snapshot_args);
    }
    let snapshot_out = run_cli_command(&bin, &snapshot_args).map_err(|e| e.to_string())?;
    let snapshot_stdout = String::from_utf8_lossy(&snapshot_out.stdout);
    if show_cli_io {
        log_cli_stdout(&plugin_id, "table snapshot", &snapshot_stdout);
    }
    if show_cli_stderr {
        let stderr = String::from_utf8_lossy(&snapshot_out.stderr);
        if !stderr.trim().is_empty() {
            log_cli_stderr(&plugin_id, "table snapshot", &stderr);
        }
    }
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
    let target_join = if let Some(n) = opts.players {
        usize::from(n)
    } else {
        vacancy
    };
    if opts.players.is_none() && vacancy == 0 {
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

    let observer_name = {
        let s = Uuid::new_v4().to_string();
        let frag = s
            .split('-')
            .next()
            .expect("uuid v4 string is hyphenated");
        format!("{}{}", observer_session_prefix(&plugin_id), frag)
    };

    let join_ctx = JoinNamesContext {
        plugin_id: plugin_id.clone(),
        table_id: table_id.clone(),
        count: target_join,
        snapshot: Some(snapshot.clone()),
    };
    let display_names: Vec<String> =
        match plugin.name_policy().join_display_names(&join_ctx) {
            Ok(v) if v.len() == target_join => v,
            _ => default_display_names_for_plugin(&plugin_id, target_join),
        };

    let mut pids: Vec<String> = Vec::new();
    for i in 0..target_join {
        let label = format!("table join bot{i}");
        let bot_name = display_names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("bot{i}"));
        let args = vec![
            "table".to_string(),
            "join".to_string(),
            "-t".to_string(),
            table_id.clone(),
            "--name".to_string(),
            bot_name,
            "--seat".to_string(),
            "auto".to_string(),
        ];
        if show_cli_io {
            log_cli_call(&plugin_id, &label, &args);
        }
        let out = run_cli_command(&bin, &args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if show_cli_io {
            log_cli_stdout(&plugin_id, &label, &stdout);
        }
        if show_cli_stderr {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.trim().is_empty() {
                log_cli_stderr(&plugin_id, &label, &stderr);
            }
        }
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
        if show_cli_io {
            log_cli_call(&plugin_id, &label, &args);
        }
        let out = run_cli_command(&bin, &args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if show_cli_io {
            log_cli_stdout(&plugin_id, &label, &stdout);
        }
        if show_cli_stderr {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.trim().is_empty() {
                log_cli_stderr(&plugin_id, &label, &stderr);
            }
        }
        let _j = parse_cli_stdout_json(&out.stdout)?;
    }

    let controlled_pids: Arc<HashSet<String>> = Arc::new(pids.iter().cloned().collect());
    let controlled_pids_text = pids.join(",");
    let shared = Arc::new(RuntimeShared {
        stop: AtomicBool::new(false),
        start_seq,
        last_scoring_transition_seq: Mutex::new(None),
        hands_done: AtomicU32::new(0),
        hands_target: opts.hands,
        err: Mutex::new(None),
    });

    let mut handles = Vec::new();

    let bin_obs = bin.clone();
    let table_id_obs = table_id.clone();
    let observer_name_obs = observer_name.clone();
    let shared_obs = Arc::clone(&shared);
    let observer_policy_obs = Arc::clone(&observer_policy);
    let verbosity_obs = opts.verbosity;
    let show_cli_io_obs = opts.show_cli_io();
    let show_cli_stderr_obs = opts.show_cli_stderr();
    let plugin_id_obs = plugin_id.clone();
    let game_start_ctx = ObserverGameStartContext {
        plugin_id: plugin_id_obs.clone(),
        table_id: table_id.clone(),
        observer_name: observer_name.clone(),
        transition_seq: 0,
        hands_target: opts.hands,
        occupied,
        vacancy,
        join_bots: target_join,
        verbosity: opts.verbosity,
    };
    handles.push(thread::spawn(move || loop {
        if shared_obs.stop.load(Ordering::SeqCst) {
            break;
        }
        let argv = vec![
            "table".to_string(),
            "nextstate".to_string(),
            "-t".to_string(),
            table_id_obs.clone(),
            "--observer-name".to_string(),
            observer_name_obs.clone(),
            "--timeout-ms".to_string(),
            NEXTSTATE_TIMEOUT_MS.to_string(),
        ];
        if show_cli_io_obs {
            log_cli_call(&plugin_id_obs, "observer", &argv);
        }
        let out = match run_cli_command(&bin_obs, &argv) {
            Ok(o) => o,
            Err(e) => {
                eprintln!(
                    "[{plugin_id_obs}][E][cli:error] actor=observer op=nextstate cmd=clawguandan {} err={e}",
                    argv.join(" ")
                );
                let msg = e.to_string();
                if msg.contains("error sending request") || msg.contains("connection") {
                    std::thread::sleep(Duration::from_millis(200));
                    match run_cli_command(&bin_obs, &argv) {
                        Ok(o) => o,
                        Err(e2) => {
                            eprintln!(
                                "[{plugin_id_obs}][E][cli:error] actor=observer op=nextstate(retry) cmd=clawguandan {} err={e2}",
                                argv.join(" ")
                            );
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
        let no_content = nextstate_stdout_is_no_content(&out.stdout);
        if show_cli_io_obs {
            if !no_content {
                log_cli_stdout(&plugin_id_obs, "observer", &out_txt);
            }
            if show_cli_stderr_obs && !err_txt.trim().is_empty() {
                log_cli_stderr(&plugin_id_obs, "observer", &err_txt);
            }
        }
        if no_content {
            continue;
        }
        let v = match parse_cli_stdout_json(&out.stdout) {
            Ok(j) => j,
            Err(e) => {
                shared_obs.fail(format!("observer: {e}"));
                break;
            }
        };
        if let Err(e) = observer_policy_obs.on_transition(&v, verbosity_obs) {
            shared_obs.fail(format!("observer plugin hook: {e}"));
            break;
        }
        let tr_type = v
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();
        if tr_type == "GAME_STARTED" {
            let mut start_ctx = game_start_ctx.clone();
            start_ctx.transition_seq = v.get("seq").and_then(|x| x.as_u64()).unwrap_or_default();
            if let Err(e) = observer_policy_obs.on_game_start(&start_ctx) {
                shared_obs.fail(format!("observer plugin hook: {e}"));
                break;
            }
        }
        if tr_type == "GAME_STARTED" || tr_type == "NEXT_HAND_STARTED" {
            let hand_index = shared_obs.hands_done.load(Ordering::SeqCst) + 1;
            let ctx = ObserverHandStartContext {
                plugin_id: plugin_id_obs.clone(),
                table_id: table_id_obs.clone(),
                hand_index,
                transition_seq: v.get("seq").and_then(|x| x.as_u64()).unwrap_or_default(),
                transition_type: tr_type.clone(),
                verbosity: verbosity_obs,
            };
            if let Err(e) = observer_policy_obs.on_hand_start(&ctx) {
                shared_obs.fail(format!("observer plugin hook: {e}"));
                break;
            }
        }
        if let Some((hand_index, tr_seq, tr_type)) = shared_obs.on_transition_maybe_scoring(&v) {
            let ctx = ObserverHandOverContext {
                plugin_id: plugin_id_obs.clone(),
                table_id: table_id_obs.clone(),
                hand_index,
                transition_seq: tr_seq,
                transition_type: tr_type.clone(),
                verbosity: verbosity_obs,
            };
            if let Err(e) = observer_policy_obs.on_hand_over(&ctx) {
                shared_obs.fail(format!("observer plugin hook: {e}"));
                break;
            }
            if tr_type == "GAME_COMPLETED" {
                let game_over_ctx = ObserverGameOverContext {
                    plugin_id: plugin_id_obs.clone(),
                    table_id: table_id_obs.clone(),
                    hands_done: hand_index,
                    transition_seq: tr_seq,
                    transition_type: tr_type.clone(),
                    verbosity: verbosity_obs,
                };
                if let Err(e) = observer_policy_obs.on_game_over(&game_over_ctx) {
                    shared_obs.fail(format!("observer plugin hook: {e}"));
                    break;
                }
            }
        }
        let terminal = shared_obs.on_transition_maybe_terminal(&v);
        if terminal && tr_type != "GAME_COMPLETED" {
            let game_over_ctx = ObserverGameOverContext {
                plugin_id: plugin_id_obs.clone(),
                table_id: table_id_obs.clone(),
                hands_done: shared_obs.hands_done.load(Ordering::SeqCst),
                transition_seq: v.get("seq").and_then(|x| x.as_u64()).unwrap_or_default(),
                transition_type: tr_type,
                verbosity: verbosity_obs,
            };
            if let Err(e) = observer_policy_obs.on_game_over(&game_over_ctx) {
                shared_obs.fail(format!("observer plugin hook: {e}"));
                break;
            }
        }
        if shared_obs.stop.load(Ordering::SeqCst) {
            break;
        }
    }));

    for (i, pid) in pids.iter().cloned().enumerate() {
        let bin_bot = bin.clone();
        let table_id_bot = table_id.clone();
        let player_name_bot = display_names
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("bot{i}"));
        let player_label_bot = format!("{pid}({player_name_bot})");
        let shared_bot = Arc::clone(&shared);
        let controlled_pids_bot = Arc::clone(&controlled_pids);
        let controlled_pids_text_bot = controlled_pids_text.clone();
        let actor_label = player_label_bot.clone();
        let plugin_bot = Arc::clone(&plugin);
        let plugin_id_bot = plugin_bot.plugin_id().to_string();
        let show_summary_bot = opts.show_summary();
        let show_cli_io_bot = opts.show_cli_io();
        let show_cli_stderr_bot = opts.show_cli_stderr();
        handles.push(thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50 * i as u64));
            let mut steps: u64 = 0;
            loop {
                if shared_bot.stop.load(Ordering::SeqCst) {
                    break;
                }
                if steps >= MAX_STEPS {
                    shared_bot.fail(format!(
                        "{actor_label}: exceeded max steps ({MAX_STEPS}); possible livelock"
                    ));
                    break;
                }
                steps += 1;

                let argv = vec![
                    "play".to_string(),
                    "wait4myturn".to_string(),
                    "-t".to_string(),
                    table_id_bot.clone(),
                    "-p".to_string(),
                    pid.clone(),
                    "--timeout-ms".to_string(),
                    BOT_WAIT4MYTURN_TIMEOUT_MS.to_string(),
                ];
                if show_cli_io_bot {
                    log_cli_call(&plugin_id_bot, &actor_label, &argv);
                }
                let out = match run_cli_command(&bin_bot, &argv) {
                    Ok(o) => o,
                    Err(e) => {
                        let msg = e.to_string();
                        if cli_error_is_wait4myturn_timeout(&msg) {
                            if shared_bot.stop.load(Ordering::SeqCst) {
                                break;
                            }
                            continue;
                        }
                        eprintln!(
                            "[{plugin_id_bot}][E][cli:error] actor={actor_label} op=wait4myturn cmd=clawguandan {} err={e}",
                            argv.join(" ")
                        );
                        if msg.contains("error sending request") || msg.contains("connection") {
                            std::thread::sleep(Duration::from_millis(200));
                            match run_cli_command(&bin_bot, &argv) {
                                Ok(o) => o,
                                Err(e2) => {
                                    let msg2 = e2.to_string();
                                    if cli_error_is_wait4myturn_timeout(&msg2) {
                                        if shared_bot.stop.load(Ordering::SeqCst) {
                                            break;
                                        }
                                        continue;
                                    }
                                    eprintln!(
                                        "[{plugin_id_bot}][E][cli:error] actor={actor_label} op=wait4myturn(retry) cmd=clawguandan {} err={e2}",
                                        argv.join(" ")
                                    );
                                    shared_bot.fail(format!("{actor_label}: wait4myturn: {e2}"));
                                    break;
                                }
                            }
                        } else {
                            shared_bot.fail(format!("{actor_label}: wait4myturn: {e}"));
                            break;
                        }
                    }
                };
                let out_txt = String::from_utf8_lossy(&out.stdout);
                let err_txt = String::from_utf8_lossy(&out.stderr);
                if show_cli_io_bot {
                    log_cli_stdout(&plugin_id_bot, &actor_label, &out_txt);
                    if show_cli_stderr_bot && !err_txt.trim().is_empty() {
                        log_cli_stderr(&plugin_id_bot, &actor_label, &err_txt);
                    }
                }

                let state = match parse_cli_stdout_json(&out.stdout) {
                    Ok(j) => j,
                    Err(e) => {
                        shared_bot.fail(format!("{actor_label}: {e}"));
                        break;
                    }
                };

                if table_state_is_terminal(&state) {
                    shared_bot.stop.store(true, Ordering::SeqCst);
                    break;
                }

                let expect = match state.get("expect") {
                    Some(e) => e.clone(),
                    None => {
                        shared_bot.fail(format!("{actor_label}: wait4myturn: missing expect"));
                        break;
                    }
                };
                let kind = expect.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                if let Some(actor) = expect_has_uncontrolled_actor(&expect, &controlled_pids_bot) {
                    shared_bot.fail(format!(
                        "{actor_label}: actor {actor} is not controlled by {}. controlled_bot_ids=[{controlled_pids_text_bot}]",
                        plugin_bot.plugin_id()
                    ));
                    break;
                }
                if !expect_requires_action(&state, &pid) {
                    if shared_bot.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    continue;
                }

                let ctx = BotTurnContext {
                    table_id: table_id_bot.clone(),
                    player_id: pid.clone(),
                    expect_kind: kind.to_string(),
                    state: state.clone(),
                };
                let decision_result = match kind {
                    "ready" => plugin_bot.ready_policy().decide_ready(&ctx),
                    "tribute" => plugin_bot.tribute_policy().decide_tribute(&ctx),
                    "exchange" => plugin_bot.exchange_policy().decide_exchange(&ctx),
                    "play" => plugin_bot.play_policy().decide_play(&ctx),
                    // Align with previous RuleBotPlugin fallback (`_ => UseSuggest`).
                    _ => plugin_bot.play_policy().decide_play(&ctx),
                };
                let decision = match decision_result {
                    Ok(d) => d,
                    Err(e) => {
                        shared_bot.fail(format!("{actor_label}: plugin decision: {e}"));
                        break;
                    }
                };
                let run_result = run_decision_action(
                    &bin_bot,
                    &table_id_bot,
                    &pid,
                    &player_label_bot,
                    &decision,
                    &actor_label,
                    &plugin_id_bot,
                    show_summary_bot,
                    show_cli_io_bot,
                    show_cli_stderr_bot,
                );
                if let Err(e) = run_result {
                    shared_bot.fail(e);
                    break;
                }

                if shared_bot.stop.load(Ordering::SeqCst) {
                    break;
                }
            }
        }));
    }

    for h in handles {
        h.join().map_err(|_| format!("{bot_label}: thread panicked"))?;
    }
    if let Some(e) = shared.err.lock().unwrap().take() {
        return Err(e);
    }

    if opts.show_summary() {
        println!(
            "=== {bot_label} done. table_id={table_id} observer_name={observer_name} start_seq={start_seq} ==="
        );
    }
    Ok(())
}

fn run_decision_action(
    bin: &std::path::Path,
    table_id: &str,
    player_id: &str,
    player_label: &str,
    decision: &BotDecision,
    prefix: &str,
    plugin_id: &str,
    show_summary: bool,
    show_cli_io: bool,
    show_cli_stderr: bool,
) -> Result<(), String> {
    match decision {
        BotDecision::Ready => {
            if show_summary {
                println!(
                    "[{plugin_id}][I][player:ready] player={} table={}",
                    player_label, table_id
                );
            }
            let argv = vec![
                "play".to_string(),
                "ready".to_string(),
                "-t".to_string(),
                table_id.to_string(),
                "-p".to_string(),
                player_id.to_string(),
            ];
            run_cli_with_log(
                bin,
                argv,
                prefix,
                plugin_id,
                show_cli_io,
                show_cli_stderr,
                "ready",
            )
        }
        BotDecision::UseSuggest => {
            let sargv = vec![
                "play".to_string(),
                "suggest".to_string(),
                "-t".to_string(),
                table_id.to_string(),
                "-p".to_string(),
                player_id.to_string(),
            ];
            let sug_out =
                run_cli_with_capture(
                    bin,
                    sargv,
                    prefix,
                    plugin_id,
                    show_cli_io,
                    show_cli_stderr,
                    "suggest",
                )?;
            let sug = parse_cli_stdout_json(&sug_out.stdout)?;
            let action_type = sug
                .get("actionType")
                .and_then(|x| x.as_str())
                .ok_or_else(|| format!("{prefix}: suggest: missing actionType"))?;
            let payload = sug.get("payload").cloned().unwrap_or(json!({}));
            let action = PlayerAction::try_from_action_type_payload(action_type, &payload)
                .map_err(|e| format!("{prefix}: suggest parse: {e}"))?;
            if show_summary {
                println!(
                    "[{plugin_id}][I][player:action] player={} table={} via=suggest action={}",
                    player_label,
                    table_id,
                    player_action_summary(&action)
                );
            }
            let argv = player_action_to_cli_argv_auto(&action, table_id, player_id);
            run_cli_with_log(
                bin,
                argv,
                prefix,
                plugin_id,
                show_cli_io,
                show_cli_stderr,
                "action",
            )
        }
        BotDecision::Action(action) => {
            if show_summary {
                println!(
                    "[{plugin_id}][I][player:action] player={} table={} via=policy action={}",
                    player_label,
                    table_id,
                    player_action_summary(action)
                );
            }
            let argv = player_action_to_cli_argv_auto(action, table_id, player_id);
            run_cli_with_log(
                bin,
                argv,
                prefix,
                plugin_id,
                show_cli_io,
                show_cli_stderr,
                "action",
            )
        }
    }

}

fn player_action_summary(action: &PlayerAction) -> String {
    match action {
        PlayerAction::Pass => "pass".to_string(),
        PlayerAction::Tribute { card } => format!("tribute:{card}"),
        PlayerAction::ReturnCard { card } => format!("return:{card}"),
        PlayerAction::Play {
            cards,
            wild_targets,
        } => {
            let mut base = format!("play:{}", cards.join(","));
            if let Some(wt) = wild_targets && !wt.is_empty() {
                base.push_str(&format!(" wild={}", wt.join(",")));
            }
            base
        }
    }
}

fn run_cli_with_log(
    bin: &std::path::Path,
    argv: Vec<String>,
    prefix: &str,
    plugin_id: &str,
    show_cli_io: bool,
    show_cli_stderr: bool,
    op: &str,
) -> Result<(), String> {
    run_cli_with_capture(bin, argv, prefix, plugin_id, show_cli_io, show_cli_stderr, op)
        .map(|_| ())
}

fn run_cli_with_capture(
    bin: &std::path::Path,
    argv: Vec<String>,
    prefix: &str,
    plugin_id: &str,
    show_cli_io: bool,
    show_cli_stderr: bool,
    op: &str,
) -> Result<std::process::Output, String> {
    if show_cli_io {
        log_cli_call(plugin_id, prefix, &argv);
    }
    let out = run_cli_command(bin, &argv).map_err(|e| {
        eprintln!(
            "[{plugin_id}][E][cli:error] actor={prefix} op={op} cmd=clawguandan {} err={e}",
            argv.join(" ")
        );
        format!("{prefix}: {op}: {e}")
    })?;
    if show_cli_io {
        log_cli_stdout(plugin_id, prefix, &String::from_utf8_lossy(&out.stdout));
    }
    if show_cli_stderr {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.trim().is_empty() {
            log_cli_stderr(plugin_id, prefix, &stderr);
        }
    }
    Ok(out)
}

fn log_cli_call(plugin_id: &str, actor: &str, argv: &[String]) {
    println!(
        "[{plugin_id}][D][cli:call] actor={actor} cmd=clawguandan {}",
        argv.join(" ")
    );
}

fn log_cli_stdout(plugin_id: &str, actor: &str, stdout: &str) {
    println!("[{plugin_id}][D][cli:stdout] actor={actor}\n{stdout}");
}

fn log_cli_stderr(plugin_id: &str, actor: &str, stderr: &str) {
    println!("[{plugin_id}][T][cli:stderr] actor={actor}\n{stderr}");
}

fn parse_cli_stdout_json(out: &[u8]) -> Result<Value, String> {
    let s = String::from_utf8_lossy(out);
    let t = s.trim();
    serde_json::from_str(t).map_err(|e| format!("invalid JSON from CLI: {e}; got: {t:?}"))
}

/// Observer session directory prefix derived from [`BotPlugin::plugin_id`]; path-safe for CLI session dirs.
fn observer_session_prefix(plugin_id: &str) -> String {
    let mut s: String = plugin_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if s.is_empty() {
        s = "bot".into();
    }
    s.make_ascii_lowercase();
    s
}

fn nextstate_stdout_is_no_content(stdout: &[u8]) -> bool {
    let t = String::from_utf8_lossy(stdout).trim().to_string();
    t.is_empty() || t.starts_with("(no new transition within timeout)")
}

fn cli_error_is_wait4myturn_timeout(msg: &str) -> bool {
    msg.contains("wait4myturn timeout after")
}

fn transition_counts_as_hand_done(v: &Value) -> bool {
    matches!(
        v.get("type").and_then(|x| x.as_str()),
        Some("HAND_ENDED_WAITING_READY" | "GAME_COMPLETED")
    )
}

fn table_state_is_terminal(v: &Value) -> bool {
    v.get("status").and_then(|x| x.as_str()) == Some("finished")
        || v.get("expect")
            .and_then(|e| e.get("kind"))
            .and_then(|x| x.as_str())
            == Some("game_over")
}

fn expect_requires_action(state: &Value, my_pid: &str) -> bool {
    let expect = state.get("expect").unwrap_or(&Value::Null);
    let kind = expect.get("kind").and_then(|x| x.as_str()).unwrap_or("");
    let actor_match = expect
        .get("actorPlayerIds")
        .and_then(|x| x.as_array())
        .map(|ids| ids.iter().any(|id| id.as_str() == Some(my_pid)))
        .unwrap_or(false);
    if !actor_match {
        return false;
    }
    matches!(kind, "play" | "tribute" | "exchange" | "ready")
}

fn expect_has_uncontrolled_actor(
    expect: &Value,
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
            if let Some(wt) = wild_targets && !wt.is_empty() {
                v.push("--wild-targets".into());
                v.push(wt.join(","));
            }
            v
        }
    }
}

