//! Run `ask_llm.sh`: write stdin (UTF-8 prompt), read stdout with wall-clock timeout.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VERBOSE_PROMPT_MAX: usize = 48_000;

/// Run `script` with `prompt` on stdin; return combined stdout (lossy UTF-8) or error.
///
/// When `verbose`, prints script path, prompt (possibly truncated), stdout, stderr, and elapsed time.
pub fn run_script_with_timeout(
    script: &Path,
    prompt: &str,
    timeout: Duration,
    verbose: bool,
    op_label: &str,
) -> Result<String, String> {
    let t0 = Instant::now();
    if verbose {
        println!(
            "\n### [llm-bot:{op_label}] ask_llm script={:?} prompt_bytes={} timeout_ms={}",
            script,
            prompt.len(),
            timeout.as_millis()
        );
        if prompt.len() <= VERBOSE_PROMPT_MAX {
            println!("<< [llm-bot:{op_label}] prompt stdin (full):\n{prompt}");
        } else {
            let head: String = prompt.chars().take(VERBOSE_PROMPT_MAX).collect();
            println!(
                "<< [llm-bot:{op_label}] prompt stdin (first {VERBOSE_PROMPT_MAX} chars):\n{head}\n<< ... truncated, total {} bytes",
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
            return Err(format!(
                "ask_llm timeout after {}ms",
                timeout.as_millis()
            ));
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
    if verbose {
        println!(
            "<< [llm-bot:{op_label}] stdout ({} bytes, {}ms):\n{}",
            stdout.len(),
            elapsed_ms,
            stdout
        );
        if !stderr.trim().is_empty() {
            println!(
                "<< [llm-bot:{op_label}] stderr:\n{}",
                stderr.trim_end()
            );
        } else {
            println!("<< [llm-bot:{op_label}] stderr: (empty)");
        }
    }
    if !status.success() {
        return Err(format!(
            "ask_llm exit {:?}: stderr={}",
            status.code(),
            stderr.trim()
        ));
    }
    if !verbose && !stderr.trim().is_empty() {
        eprintln!("[llm-bot] ask_llm stderr: {}", stderr.trim());
    }
    Ok(stdout)
}
