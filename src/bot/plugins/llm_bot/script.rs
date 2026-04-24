//! Run llm-bot script: write stdin (UTF-8 prompt), read stdout with wall-clock timeout.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VERBOSE_PROMPT_MAX: usize = 48_000;

fn llm_log_prefix(op_label: &str) -> String {
    format!("[llm-bot][D][llm:{op_label}]")
}

fn last_marker(stdout: &str) -> Option<String> {
    let mut cursor = 0usize;
    let mut out: Option<String> = None;
    while let Some(rel_start) = stdout[cursor..].find("<<<") {
        let start = cursor + rel_start;
        let body_start = start + 3;
        let Some(rel_end) = stdout[body_start..].find(">>>") else {
            break;
        };
        let end = body_start + rel_end + 3;
        out = Some(stdout[start..end].trim().to_string());
        cursor = end;
    }
    out
}

/// Run `script` with `prompt` on stdin; return combined stdout (lossy UTF-8) or error.
///
/// At `-v`, prints invocation lifecycle logs and **full script stdout** (model “thinking”).
/// At `-vv` and above, also prints the full stdin prompt and stderr.
pub fn run_script_with_timeout(
    script: &Path,
    prompt: &str,
    timeout: Duration,
    verbosity: u8,
    op_label: &str,
) -> Result<String, String> {
    let t0 = Instant::now();
    let log_prefix = llm_log_prefix(op_label);
    if verbosity >= 1 {
        println!(
            "{log_prefix} script={:?} prompt_bytes={} timeout_ms={} phase=start",
            script,
            prompt.len(),
            timeout.as_millis()
        );
    }
    if verbosity >= 2 {
        if prompt.len() <= VERBOSE_PROMPT_MAX {
            println!("{log_prefix} prompt_stdin_full:\n{prompt}");
        } else {
            let head: String = prompt.chars().take(VERBOSE_PROMPT_MAX).collect();
            println!(
                "{log_prefix} prompt_stdin_first_chars({VERBOSE_PROMPT_MAX}):\n{head}\n{log_prefix} prompt_stdin_truncated total_bytes={}",
                prompt.len()
            );
        }
    }

    let mut child = Command::new(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {:?}: {e}", script))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| format!("stdin: {e}"))?;
    }

    let deadline = Instant::now() + timeout;
    let status = loop {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("script timeout after {}ms", timeout.as_millis()));
        }
        match child.try_wait().map_err(|e| e.to_string())? {
            None => std::thread::sleep(Duration::from_millis(25)),
            Some(s) => break s,
        }
    };

    let mut stdout_buf = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut stdout_buf);
    }
    let mut stderr_buf = Vec::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_end(&mut stderr_buf);
    }

    let stdout = String::from_utf8_lossy(&stdout_buf).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_buf);
    let elapsed_ms = t0.elapsed().as_millis();
    let marker = last_marker(&stdout).unwrap_or_else(|| "(no-marker)".to_string());
    if verbosity >= 1 {
        println!(
            "{log_prefix} exit={:?} elapsed_ms={} stdout_bytes={} stderr_bytes={} marker={}",
            status.code(),
            elapsed_ms,
            stdout.len(),
            stderr.len(),
            marker
        );
        println!(
            "{log_prefix} stdout_full bytes={} elapsed_ms={}:\n{}",
            stdout.len(),
            elapsed_ms,
            stdout
        );
    }
    if verbosity >= 2 {
        if !stderr.trim().is_empty() {
            println!("{log_prefix} stderr:\n{}", stderr.trim_end());
        } else {
            println!("{log_prefix} stderr: (empty)");
        }
    }
    if !status.success() {
        return Err(format!(
            "script exit {:?}: stderr={}",
            status.code(),
            stderr.trim()
        ));
    }
    if verbosity == 0 && !stderr.trim().is_empty() {
        eprintln!("[llm-bot] script stderr: {}", stderr.trim());
    }
    Ok(stdout)
}
