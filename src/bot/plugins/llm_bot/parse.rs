//! Parse llm-bot script stdout: `<<<DECISION:...>>>`, `<<<NAMING:LIST|...>>>`, `<<<DEFAULT>>>`.

use regex::Regex;
use serde_json::{Value, json};

use crate::bot::plugin::BotDecision;
use crate::game::engine::PlayerAction;

const DEFAULT_TOKEN: &str = "<<<DEFAULT>>>";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParsedStdoutDecision {
    /// Use the same downgrade path as parse failure / timeout.
    Default,
    Ready,
    UseSuggest,
    Pass,
    Action(PlayerAction),
    Malformed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParsedStdoutNaming {
    Default,
    Names(Vec<String>),
    Malformed(String),
}

#[derive(Clone, Debug)]
struct Span {
    start: usize,
    kind: SpanKind,
}

#[derive(Clone, Debug)]
enum SpanKind {
    Default,
    Decision { kind: String, payload: String },
    NamingList { payload: String },
}

fn collect_spans(stdout: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();

    for (idx, _) in stdout.match_indices(DEFAULT_TOKEN) {
        out.push(Span {
            start: idx,
            kind: SpanKind::Default,
        });
    }

    // Optional `|payload`: empty kinds (READY, PASS, USE-SUGGEST) may use `<<<DECISION:PASS>>>` with no `|`.
    let re_decision =
        Regex::new(r"<<<DECISION:([A-Za-z0-9_-]+)(?:\|(.*))?>>>").expect("valid DECISION regex");
    for cap in re_decision.captures_iter(stdout) {
        let m = cap.get(0).expect("whole match");
        let kind = cap
            .get(1)
            .map(|x| x.as_str().to_string())
            .unwrap_or_default();
        let payload = cap
            .get(2)
            .map(|x| x.as_str().to_string())
            .unwrap_or_default();
        out.push(Span {
            start: m.start(),
            kind: SpanKind::Decision { kind, payload },
        });
    }

    let re_naming = Regex::new(r"<<<NAMING:LIST\|(.*?)>>>").expect("valid NAMING regex");
    for cap in re_naming.captures_iter(stdout) {
        let m = cap.get(0).expect("whole match");
        let payload = cap
            .get(1)
            .map(|x| x.as_str().to_string())
            .unwrap_or_default();
        out.push(Span {
            start: m.start(),
            kind: SpanKind::NamingList { payload },
        });
    }

    out.sort_by_key(|s| s.start);
    out
}

fn last_span(stdout: &str, allow_naming: bool) -> Option<SpanKind> {
    let mut spans = collect_spans(stdout);
    if !allow_naming {
        spans.retain(|s| !matches!(s.kind, SpanKind::NamingList { .. }));
    }
    spans.pop().map(|s| s.kind)
}

fn last_span_for_naming(stdout: &str) -> Option<SpanKind> {
    let mut spans = collect_spans(stdout);
    spans.retain(|s| matches!(s.kind, SpanKind::Default | SpanKind::NamingList { .. }));
    spans.pop().map(|s| s.kind)
}

/// Parse stdout from a **decision** script invocation (ignores `<<<NAMING:...>>>`).
pub fn parse_decision_stdout(stdout: &str) -> ParsedStdoutDecision {
    match last_span(stdout, false) {
        None => ParsedStdoutDecision::Malformed("no DECISION or DEFAULT token".into()),
        Some(SpanKind::Default) => ParsedStdoutDecision::Default,
        Some(SpanKind::NamingList { .. }) => {
            ParsedStdoutDecision::Malformed("unexpected NAMING token in decision parse".into())
        }
        Some(SpanKind::Decision { kind, payload }) => parse_decision_body(&kind, &payload),
    }
}

/// Parse a JSON object payload; if strict parse fails, retry with narrow `{` / `}` repairs
/// (common when the model drops the closing `}` before the `>>>` marker).
fn parse_decision_json_value(payload: &str) -> Result<Value, String> {
    let t = payload.trim();
    if t.is_empty() {
        return Err("empty JSON payload".into());
    }

    let mut candidates: Vec<String> = Vec::new();
    candidates.push(t.to_string());
    if t.starts_with('{') && !t.ends_with('}') {
        candidates.push(format!("{t}}}"));
    }
    if !t.starts_with('{') && t.ends_with('}') {
        let mut s = String::with_capacity(t.len() + 1);
        s.push('{');
        s.push_str(t);
        candidates.push(s);
    }

    let mut last_err = String::new();
    for s in candidates {
        match serde_json::from_str::<Value>(&s) {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

fn parse_decision_body(kind_raw: &str, payload: &str) -> ParsedStdoutDecision {
    let kind = kind_raw.trim();
    let kind_upper: String = kind.to_ascii_uppercase();
    match kind_upper.as_str() {
        "READY" => {
            if !payload.trim().is_empty() {
                return ParsedStdoutDecision::Malformed("READY expects empty payload".into());
            }
            ParsedStdoutDecision::Ready
        }
        "PASS" => {
            if !payload.trim().is_empty() {
                return ParsedStdoutDecision::Malformed("PASS expects empty payload".into());
            }
            ParsedStdoutDecision::Pass
        }
        "USE-SUGGEST" | "USESUGGEST" => {
            if !payload.trim().is_empty() {
                return ParsedStdoutDecision::Malformed("USE-SUGGEST expects empty payload".into());
            }
            ParsedStdoutDecision::UseSuggest
        }
        "PLAY" => match parse_decision_json_value(payload) {
            Ok(v) => match PlayerAction::try_from_action_type_payload("play", &v) {
                Ok(a) => ParsedStdoutDecision::Action(a),
                Err(e) => ParsedStdoutDecision::Malformed(format!("PLAY: {e}")),
            },
            Err(e) => ParsedStdoutDecision::Malformed(format!("PLAY JSON: {e}")),
        },
        "TRIBUTE" => match parse_decision_json_value(payload) {
            Ok(v) => match PlayerAction::try_from_action_type_payload("tribute", &v) {
                Ok(a) => ParsedStdoutDecision::Action(a),
                Err(e) => ParsedStdoutDecision::Malformed(format!("TRIBUTE: {e}")),
            },
            Err(e) => ParsedStdoutDecision::Malformed(format!("TRIBUTE JSON: {e}")),
        },
        "RETURN-CARD" | "RETURN_CARD" => match parse_decision_json_value(payload) {
            Ok(v) => match PlayerAction::try_from_action_type_payload("return_card", &v) {
                Ok(a) => ParsedStdoutDecision::Action(a),
                Err(e) => ParsedStdoutDecision::Malformed(format!("RETURN-CARD: {e}")),
            },
            Err(e) => ParsedStdoutDecision::Malformed(format!("RETURN-CARD JSON: {e}")),
        },
        _ => ParsedStdoutDecision::Malformed(format!("unknown DECISION kind {:?}", kind)),
    }
}

/// Parse stdout from a **naming** script invocation (only `DEFAULT` or `NAMING:LIST`).
pub fn parse_naming_stdout(stdout: &str) -> ParsedStdoutNaming {
    match last_span_for_naming(stdout) {
        None => ParsedStdoutNaming::Malformed("no NAMING or DEFAULT token".into()),
        Some(SpanKind::Default) => ParsedStdoutNaming::Default,
        Some(SpanKind::Decision { .. }) => {
            ParsedStdoutNaming::Malformed("unexpected DECISION token in naming parse".into())
        }
        Some(SpanKind::NamingList { payload }) => {
            let trimmed = payload.trim();
            let v: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => return ParsedStdoutNaming::Malformed(format!("NAMING JSON: {e}")),
            };
            let names: Vec<String> = match v.get("names").and_then(|x| x.as_array()) {
                Some(arr) => arr
                    .iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect(),
                None => return ParsedStdoutNaming::Malformed("missing names array".into()),
            };
            ParsedStdoutNaming::Names(names)
        }
    }
}

/// Map parsed token + `expect_kind` into [`BotDecision`], applying `DEFAULT` / malformed downgrade.
pub fn parsed_decision_to_bot_decision(
    parsed: ParsedStdoutDecision,
    expect_kind: &str,
) -> BotDecision {
    match parsed {
        ParsedStdoutDecision::Default | ParsedStdoutDecision::Malformed(_) => {
            fallback_decision(expect_kind)
        }
        ParsedStdoutDecision::Ready => BotDecision::Ready,
        ParsedStdoutDecision::UseSuggest => BotDecision::UseSuggest,
        ParsedStdoutDecision::Pass => BotDecision::Action(PlayerAction::Pass),
        ParsedStdoutDecision::Action(a) => BotDecision::Action(a),
    }
}

pub fn fallback_decision(expect_kind: &str) -> BotDecision {
    match expect_kind {
        "ready" => BotDecision::Ready,
        "play" | "tribute" | "exchange" => BotDecision::UseSuggest,
        _ => BotDecision::UseSuggest,
    }
}

/// If action is inconsistent with `state.expect.legalActions` / `private.playHints`, downgrade to suggest when legal.
pub fn validate_decision_against_state(decision: BotDecision, state: &Value) -> BotDecision {
    let expect = state.get("expect").cloned().unwrap_or(json!({}));
    let legal: Vec<String> = expect
        .get("legalActions")
        .and_then(|x| x.as_array())
        .map(|xs| {
            xs.iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();
    let can_pass = legal.iter().any(|s| s == "pass");
    let can_play = legal.iter().any(|s| s == "play");

    match &decision {
        BotDecision::Action(PlayerAction::Pass) => {
            if can_pass {
                decision
            } else {
                BotDecision::UseSuggest
            }
        }
        BotDecision::Action(PlayerAction::Play { .. }) => {
            if can_play {
                let hints_ok = state
                    .get("private")
                    .and_then(|p| p.get("playHints"))
                    .map(|h| h.get("canPlay").and_then(|x| x.as_bool()).unwrap_or(true))
                    .unwrap_or(true);
                if hints_ok {
                    decision
                } else {
                    if can_pass {
                        BotDecision::Action(PlayerAction::Pass)
                    } else {
                        BotDecision::UseSuggest
                    }
                }
            } else if can_pass {
                BotDecision::Action(PlayerAction::Pass)
            } else {
                BotDecision::UseSuggest
            }
        }
        BotDecision::Action(PlayerAction::Tribute { .. }) => {
            if legal.iter().any(|s| s == "tribute") {
                decision
            } else {
                BotDecision::UseSuggest
            }
        }
        BotDecision::Action(PlayerAction::ReturnCard { .. }) => {
            if legal
                .iter()
                .any(|s| s == "return_card" || s == "returnCard")
            {
                decision
            } else {
                BotDecision::UseSuggest
            }
        }
        _ => decision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_default_last_wins() {
        let s = "thinking\n<<<DECISION:PASS|>>>\n<<<DEFAULT>>>";
        let p = parse_decision_stdout(s);
        assert!(matches!(p, ParsedStdoutDecision::Default));
    }

    #[test]
    fn decision_pass_parsed() {
        let s = "x <<<DECISION:PASS|>>>";
        assert!(matches!(
            parse_decision_stdout(s),
            ParsedStdoutDecision::Pass
        ));
    }

    #[test]
    fn decision_pass_no_pipe() {
        assert!(matches!(
            parse_decision_stdout("<<<DECISION:PASS>>>"),
            ParsedStdoutDecision::Pass
        ));
    }

    #[test]
    fn decision_ready_no_pipe() {
        assert!(matches!(
            parse_decision_stdout("<<<DECISION:READY>>>"),
            ParsedStdoutDecision::Ready
        ));
    }

    #[test]
    fn decision_play_json() {
        let s = r##"<<<DECISION:PLAY|{"cards":["♠3"]}>>>"##;
        let p = parse_decision_stdout(s);
        assert!(matches!(
            p,
            ParsedStdoutDecision::Action(PlayerAction::Play { .. })
        ));
    }

    #[test]
    fn decision_play_json_missing_closing_brace() {
        let s = r##"<<<DECISION:PLAY|{"cards":["♣Q"]>>>"##;
        let p = parse_decision_stdout(s);
        assert!(matches!(
            p,
            ParsedStdoutDecision::Action(PlayerAction::Play { .. })
        ));
    }

    #[test]
    fn naming_list() {
        let s = r##"<<<NAMING:LIST|{"names":["a","b"]}>>>"##;
        match parse_naming_stdout(s) {
            ParsedStdoutNaming::Names(n) => assert_eq!(n, vec!["a", "b"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn naming_default() {
        assert!(matches!(
            parse_naming_stdout("<<<NAMING:LIST|{}>>>\n<<<DEFAULT>>>"),
            ParsedStdoutNaming::Default
        ));
    }

    #[test]
    fn decision_ignores_naming_span() {
        let s = "<<<NAMING:LIST|{\"names\":[\"x\"]}>>>\n<<<DECISION:USE-SUGGEST|>>>";
        let p = parse_decision_stdout(s);
        assert!(matches!(p, ParsedStdoutDecision::UseSuggest));
    }
}
