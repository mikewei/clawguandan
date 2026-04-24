use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Self-check script by asking it to report the concrete model marker.
pub fn verify_script_model(script: &Path, timeout: Duration) -> Result<String, String> {
    const EXPECTED_MARKER_EXAMPLE: &str = "<<<MODEL:gpt-5>>>";
    let prompt = format!(
        "Self-check. Tell me the exact model name you are currently using.\n\
Reply with exactly one marker line and nothing else:\n{EXPECTED_MARKER_EXAMPLE}\n"
    );
    let out = run_script_for_check(script, &prompt, timeout)?;
    parse_model_marker_from_stdout(&out).ok_or_else(|| {
        format!(
            "expected marker like {EXPECTED_MARKER_EXAMPLE}, got stdout={:?}",
            out.trim()
        )
    })
}

pub fn resolve_join_model(
    explicit_model: Option<String>,
    detected_model: String,
) -> Option<String> {
    let explicit = explicit_model.and_then(|m| {
        let t = m.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    });
    if explicit.is_some() {
        return explicit;
    }
    let t = detected_model.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn parse_model_marker_from_stdout(stdout: &str) -> Option<String> {
    let mut cursor = 0usize;
    let mut out: Option<String> = None;
    while let Some(rel_start) = stdout[cursor..].find("<<<MODEL:") {
        let start = cursor + rel_start + "<<<MODEL:".len();
        let tail = &stdout[start..];
        let Some(rel_end) = tail.find(">>>") else {
            break;
        };
        let raw = &tail[..rel_end];
        let m = raw.trim();
        if !m.is_empty() {
            out = Some(m.to_string());
        }
        cursor = start + rel_end + ">>>".len();
    }
    out
}

fn run_script_for_check(script: &Path, prompt: &str, timeout: Duration) -> Result<String, String> {
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

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timeout".into());
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
    let stderr = String::from_utf8_lossy(&stderr_buf).into_owned();
    if !status.success() {
        return Err(format!(
            "script exit {:?}: stderr={}",
            status.code(),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&stdout_buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_marker_last_wins() {
        let s = "x<<<MODEL:gpt-4o>>>y\n<<<MODEL:gpt-5-mini>>>";
        assert_eq!(
            parse_model_marker_from_stdout(s).as_deref(),
            Some("gpt-5-mini")
        );
    }

    #[test]
    fn resolve_join_model_prefers_explicit_value() {
        assert_eq!(
            resolve_join_model(Some("  explicit-x ".into()), "auto-y".into()).as_deref(),
            Some("explicit-x")
        );
        assert_eq!(
            resolve_join_model(None, " auto-y ".into()).as_deref(),
            Some("auto-y")
        );
    }
}
