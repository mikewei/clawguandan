//! LLM batch naming: prompt, script, parse, sanitize/dedupe.

use std::path::Path;
use std::time::Duration;

use crate::bot::plugin::JoinNamesContext;

use super::parse::{ParsedStdoutNaming, parse_naming_stdout};
use super::prompt;
use super::script::run_script_with_timeout;

const MAX_NAME_LEN: usize = 24;

fn sanitize_one(s: &str) -> String {
    let t = s.trim();
    let mut out = String::new();
    for ch in t.chars().take(MAX_NAME_LEN) {
        if ch.is_control() || ch == '/' || ch == '\\' {
            continue;
        }
        out.push(ch);
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        "bot".into()
    } else {
        out
    }
}

fn dedupe_names(mut names: Vec<String>) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for n in &mut names {
        let base = n.clone();
        let mut k = 2u32;
        while seen.contains(n.as_str()) {
            let suf = format!("-{k}");
            let keep = MAX_NAME_LEN.saturating_sub(suf.len());
            let prefix: String = base.chars().take(keep).collect();
            *n = format!("{prefix}{suf}");
            k += 1;
        }
        seen.insert(n.clone());
    }
    names
}

pub fn resolve(
    script: &Path,
    timeout: Duration,
    verbose: bool,
    ctx: &JoinNamesContext,
) -> Result<Vec<String>, String> {
    if verbose {
        println!(
            "[llm-bot] naming: table={} count={}",
            ctx.table_id, ctx.count
        );
    }
    let prompt = prompt::naming_prompt(&ctx.table_id, ctx.count, ctx.snapshot.as_ref());
    let stdout = run_script_with_timeout(script, &prompt, timeout, verbose, "naming")?;
    let parsed = parse_naming_stdout(&stdout);
    if verbose {
        println!("[llm-bot] naming: parsed = {parsed:?}");
    }
    match parsed {
        ParsedStdoutNaming::Default => Err("<<<DEFAULT>>>".into()),
        ParsedStdoutNaming::Malformed(e) => Err(e),
        ParsedStdoutNaming::Names(names) => {
            if names.len() != ctx.count {
                return Err(format!(
                    "expected {} names, got {}",
                    ctx.count,
                    names.len()
                ));
            }
            let sanitized: Vec<String> = names.iter().map(|s| sanitize_one(s)).collect();
            let out = dedupe_names(sanitized);
            if verbose {
                println!("[llm-bot] naming: final display names = {out:?}");
            }
            Ok(out)
        }
    }
}
