use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::bot::plugin::{BotDecision, BotPlugin, BotTurnContext};
use crate::domain::TableState;
use crate::game::engine::PlayerAction;
use crate::simulation::run_cli_command;

const NEXTSTATE_TIMEOUT_MS: u64 = 110_000;
const MAX_STEPS: u64 = 500_000;

#[derive(Clone, Debug)]
pub struct BotRunOptions {
    pub table: Option<String>,
    pub rank: Option<String>,
    pub players: Option<u8>,
    pub hands: u32,
    pub verbose: bool,
}

struct RuntimeShared {
    stop: AtomicBool,
    start_seq: u64,
    last_scoring_transition_seq: Mutex<Option<u64>>,
    hands_done: AtomicU32,
    hands_target: u32,
    err: Mutex<Option<String>>,
    bot_label: String,
}

impl RuntimeShared {
    fn fail(&self, msg: String) {
        let mut e = self.err.lock().unwrap();
        if e.is_none() {
            *e = Some(msg);
        }
        self.stop.store(true, Ordering::SeqCst);
    }

    fn on_transition_maybe_scoring(&self, v: &Value) {
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
        println!(
            "\n--- {}: hand {n} completed (transition seq={tr_seq}) ---",
            self.bot_label
        );
        if n >= self.hands_target {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn on_transition_maybe_terminal(&self, v: &Value) {
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

pub fn run_bot_subprocess(
    opts: BotRunOptions,
    plugin: Arc<dyn BotPlugin>,
) -> Result<(), String> {
    if opts.hands == 0 {
        return Err("--hands must be >= 1".into());
    }
    if let Some(n) = opts.players && n > 4 {
        return Err("--players must be <= 4".into());
    }

    let bot_label = format!("bot {}", plugin.name());
    let bin = std::env::current_exe().map_err(|e| e.to_string())?;
    if opts.verbose {
        println!(
            "--- {bot_label}: hands={} (observer + bots; subprocess CLI; auto-seq) ---",
            opts.hands
        );
    } else {
        println!(
            "--- {bot_label}: hands={} (compact log; use -v for subprocess I/O) ---",
            opts.hands
        );
    }

    let table_id = if let Some(tid) = opts.table.clone() {
        if opts.rank.is_some() {
            return Err("--rank is only allowed when creating a new table (omit --table)".into());
        }
        if opts.verbose {
            println!("\n### [table target]\nusing existing table: {tid}");
        } else {
            println!("--- {bot_label}: using existing table_id={tid} ---");
        }
        tid
    } else {
        let label = "table create";
        let mut create_args = vec![
            "table".to_string(),
            "create".to_string(),
            format!("bot-{}", plugin.name()),
        ];
        if let Some(rank) = opts.rank.as_deref() {
            create_args.push("--rank".to_string());
            create_args.push(rank.to_string());
        }
        if opts.verbose {
            println!("\n### [{label}]\n$ clawguandan {}", create_args.join(" "));
        }
        let out = run_cli_command(&bin, &create_args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if opts.verbose {
            println!("<< stdout:\n{stdout}");
        }
        let create_v = parse_cli_stdout_json(&out.stdout)?;
        let tid = create_v["tableId"]
            .as_str()
            .or_else(|| create_v["table_id"].as_str())
            .ok_or_else(|| "create: missing tableId".to_string())?
            .to_string();
        if !opts.verbose {
            println!("--- {bot_label}: created table_id={tid} ---");
        }
        tid
    };

    let snapshot_args = vec![
        "table".to_string(),
        "snapshot".to_string(),
        "-t".to_string(),
        table_id.clone(),
    ];
    if opts.verbose {
        println!(
            "\n### [table snapshot]\n$ clawguandan {}",
            snapshot_args.join(" ")
        );
    }
    let snapshot_out = run_cli_command(&bin, &snapshot_args).map_err(|e| e.to_string())?;
    let snapshot_stdout = String::from_utf8_lossy(&snapshot_out.stdout);
    if opts.verbose {
        println!("<< stdout:\n{snapshot_stdout}");
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
    println!(
        "--- {bot_label}: table_id={table_id} occupied={occupied} vacancy={vacancy} join_bots={target_join} ---"
    );

    let observer_name = {
        let s = Uuid::new_v4().to_string();
        let frag = s
            .split('-')
            .next()
            .expect("uuid v4 string is hyphenated");
        format!("{}{}", plugin.observer_prefix(), frag)
    };
    println!("--- {bot_label}: observer_session={observer_name} ---");

    let last_narration_raw: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    if !opts.verbose {
        let raw = snapshot_state.narration.trim();
        if !raw.is_empty() {
            let line = narration_display_en(raw);
            if !line.is_empty() {
                println!("[narration] {line}");
                *last_narration_raw.lock().unwrap() = Some(raw.to_string());
            }
        }
    }

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
        if opts.verbose {
            println!("\n### [{label}]\n$ clawguandan {}", args.join(" "));
        }
        let out = run_cli_command(&bin, &args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if opts.verbose {
            println!("<< stdout:\n{stdout}");
        }
        let j = parse_cli_stdout_json(&out.stdout)?;
        let pid = j["playerId"]
            .as_str()
            .ok_or_else(|| "join: missing playerId".to_string())?
            .to_string();
        pids.push(pid);
    }
    if !opts.verbose && !pids.is_empty() {
        println!(
            "--- {bot_label}: joined {} bot(s), sent ready ---",
            pids.len()
        );
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
        if opts.verbose {
            println!("\n### [{label}]\n$ clawguandan {}", args.join(" "));
        }
        let out = run_cli_command(&bin, &args).map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if opts.verbose {
            println!("<< stdout:\n{stdout}");
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
        bot_label: bot_label.clone(),
    });

    let mut handles = Vec::new();

    let bin_obs = bin.clone();
    let table_id_obs = table_id.clone();
    let observer_name_obs = observer_name.clone();
    let shared_obs = Arc::clone(&shared);
    let last_narr_obs = Arc::clone(&last_narration_raw);
    let plugin_obs = Arc::clone(&plugin);
    let verbose_obs = opts.verbose;
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
        if verbose_obs {
            println!("\n### [observer] $ clawguandan {}", argv.join(" "));
        }
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
        if verbose_obs {
            println!("<< [observer] stdout:\n{out_txt}");
            if !err_txt.trim().is_empty() {
                println!("<< [observer] stderr:\n{err_txt}");
            }
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
        if let Err(e) = plugin_obs.on_observer_transition(&v) {
            shared_obs.fail(format!("observer plugin hook: {e}"));
            break;
        }
        if !verbose_obs && let Some(raw) = last_narration_from_nextstate_json(&v) {
            let mut g = last_narr_obs.lock().unwrap();
            let changed = g.as_ref().map(|s| s.as_str()) != Some(raw.as_str());
            if changed {
                let disp = narration_display_en(&raw);
                if !disp.is_empty() {
                    println!("[narration] {disp}");
                }
                *g = Some(raw);
            }
        }
        shared_obs.on_transition_maybe_scoring(&v);
        shared_obs.on_transition_maybe_terminal(&v);
        if shared_obs.stop.load(Ordering::SeqCst) {
            break;
        }
    }));

    for (i, pid) in pids.iter().cloned().enumerate() {
        let bin_bot = bin.clone();
        let table_id_bot = table_id.clone();
        let shared_bot = Arc::clone(&shared);
        let controlled_pids_bot = Arc::clone(&controlled_pids);
        let controlled_pids_text_bot = controlled_pids_text.clone();
        let prefix = format!("bot{i}");
        let verbose_bot = opts.verbose;
        let plugin_bot = Arc::clone(&plugin);
        handles.push(thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50 * i as u64));
            let mut steps: u64 = 0;
            loop {
                if shared_bot.stop.load(Ordering::SeqCst) {
                    break;
                }
                if steps >= MAX_STEPS {
                    shared_bot.fail(format!(
                        "{prefix}: exceeded max steps ({MAX_STEPS}); possible livelock"
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
                    NEXTSTATE_TIMEOUT_MS.to_string(),
                ];
                if verbose_bot {
                    println!("\n### [{prefix}] $ clawguandan {}", argv.join(" "));
                }
                let out = match run_cli_command(&bin_bot, &argv) {
                    Ok(o) => o,
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("error sending request") || msg.contains("connection") {
                            std::thread::sleep(Duration::from_millis(200));
                            match run_cli_command(&bin_bot, &argv) {
                                Ok(o) => o,
                                Err(e2) => {
                                    shared_bot.fail(format!("{prefix}: wait4myturn: {e2}"));
                                    break;
                                }
                            }
                        } else {
                            shared_bot.fail(format!("{prefix}: wait4myturn: {e}"));
                            break;
                        }
                    }
                };
                let out_txt = String::from_utf8_lossy(&out.stdout);
                let err_txt = String::from_utf8_lossy(&out.stderr);
                if verbose_bot {
                    println!("<< [{prefix}] stdout:\n{out_txt}");
                    if !err_txt.trim().is_empty() {
                        println!("<< [{prefix}] stderr:\n{err_txt}");
                    }
                }

                let state = match parse_cli_stdout_json(&out.stdout) {
                    Ok(j) => j,
                    Err(e) => {
                        shared_bot.fail(format!("{prefix}: {e}"));
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
                        shared_bot.fail(format!("{prefix}: wait4myturn: missing expect"));
                        break;
                    }
                };
                let kind = expect.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                if let Some(actor) = expect_has_uncontrolled_actor(&expect, &controlled_pids_bot) {
                    shared_bot.fail(format!(
                        "{prefix}: actor {actor} is not controlled by {}. controlled_bot_ids=[{controlled_pids_text_bot}]",
                        plugin_bot.name()
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
                let decision = match plugin_bot.decide(&ctx) {
                    Ok(d) => d,
                    Err(e) => {
                        shared_bot.fail(format!("{prefix}: plugin decision: {e}"));
                        break;
                    }
                };
                let run_result = run_decision_action(
                    &bin_bot,
                    &table_id_bot,
                    &pid,
                    &decision,
                    &prefix,
                    verbose_bot,
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

    println!(
        "\n=== {bot_label} done. table_id={table_id} observer_name={observer_name} start_seq={start_seq} ==="
    );
    Ok(())
}

fn run_decision_action(
    bin: &std::path::Path,
    table_id: &str,
    player_id: &str,
    decision: &BotDecision,
    prefix: &str,
    verbose: bool,
) -> Result<(), String> {
    match decision {
        BotDecision::Ready => {
            let argv = vec![
                "play".to_string(),
                "ready".to_string(),
                "-t".to_string(),
                table_id.to_string(),
                "-p".to_string(),
                player_id.to_string(),
            ];
            run_cli_with_log(bin, argv, prefix, verbose, "ready")
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
            let sug_out = run_cli_with_capture(bin, sargv, prefix, verbose, "suggest")?;
            let sug = parse_cli_stdout_json(&sug_out.stdout)?;
            let action_type = sug
                .get("actionType")
                .and_then(|x| x.as_str())
                .ok_or_else(|| format!("{prefix}: suggest: missing actionType"))?;
            let payload = sug.get("payload").cloned().unwrap_or(json!({}));
            let action = PlayerAction::try_from_action_type_payload(action_type, &payload)
                .map_err(|e| format!("{prefix}: suggest parse: {e}"))?;
            let argv = player_action_to_cli_argv_auto(&action, table_id, player_id);
            run_cli_with_log(bin, argv, prefix, verbose, "action")
        }
        BotDecision::Action(action) => {
            let argv = player_action_to_cli_argv_auto(action, table_id, player_id);
            run_cli_with_log(bin, argv, prefix, verbose, "action")
        }
    }
}

fn run_cli_with_log(
    bin: &std::path::Path,
    argv: Vec<String>,
    prefix: &str,
    verbose: bool,
    op: &str,
) -> Result<(), String> {
    run_cli_with_capture(bin, argv, prefix, verbose, op).map(|_| ())
}

fn run_cli_with_capture(
    bin: &std::path::Path,
    argv: Vec<String>,
    prefix: &str,
    verbose: bool,
    op: &str,
) -> Result<std::process::Output, String> {
    if verbose {
        println!("\n### [{prefix}] $ clawguandan {}", argv.join(" "));
    }
    let out = run_cli_command(bin, &argv).map_err(|e| format!("{prefix}: {op}: {e}"))?;
    if verbose {
        println!("<< [{prefix}] stdout:\n{}", String::from_utf8_lossy(&out.stdout));
    }
    Ok(out)
}

fn parse_cli_stdout_json(out: &[u8]) -> Result<Value, String> {
    let s = String::from_utf8_lossy(out);
    let t = s.trim();
    serde_json::from_str(t).map_err(|e| format!("invalid JSON from CLI: {e}; got: {t:?}"))
}

fn nextstate_stdout_is_no_content(stdout: &[u8]) -> bool {
    let t = String::from_utf8_lossy(stdout).trim().to_string();
    t.is_empty() || t.starts_with("(no new transition within timeout)")
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

fn last_narration_from_nextstate_json(v: &Value) -> Option<String> {
    let ops = v.get("delta")?.get("ops")?.as_array()?;
    let mut out: Option<String> = None;
    for op in ops {
        if op.get("op").and_then(|x| x.as_str()) == Some("replace")
            && op.get("path").and_then(|x| x.as_str()) == Some("/narration")
            && let Some(val) = op.get("value")
        {
            out = Some(match val {
                Value::String(s) => s.clone(),
                _ => val.to_string(),
            });
        }
    }
    out
}

fn narration_display_en(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(t)
        && let Some(en) = v.get("en").and_then(|x| x.as_str())
    {
        return en.trim().to_string();
    }
    t.to_string()
}
