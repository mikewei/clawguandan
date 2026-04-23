//! Free-text prompts for `ask_llm.sh` (do not mention `<<<DEFAULT>>>` to the model).

use serde_json::Value;

pub fn decision_prompt(expect_kind: &str, state: &Value) -> String {
    let state_pretty = serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".into());
    format!(
        r#"You are a Guan Dan (掼蛋) table bot. Current step kind: {expect_kind}.

Table + private JSON (materialized):
{state_pretty}

Reply with analysis if you want, then output exactly ONE machine-readable line at the end of your reply, using this format (no other <<<...>>> markers):
<<<DECISION:KIND|payload>>>  or when there is no payload: <<<DECISION:KIND>>>

Where KIND is one of: READY, PASS, USE-SUGGEST, PLAY, TRIBUTE, RETURN-CARD.
- For READY, PASS, USE-SUGGEST: no payload; you may write <<<DECISION:PASS>>> or <<<DECISION:PASS|>>> (empty after the pipe).
- For PLAY, TRIBUTE, RETURN-CARD: include a single-line JSON payload after `|`, e.g. <<<DECISION:PLAY|{{"cards":["♠3","♥3"]}}>>> or with declaredWildMapping as in the game API.

Choose a legal action consistent with expect.legalActions and your private hand when visible."#
    )
}

pub fn naming_prompt(table_id: &str, count: usize, snapshot: Option<&Value>) -> String {
    let snap = snapshot
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into()))
        .unwrap_or_else(|| "(none)".into());
    format!(
        r#"You name {count} bot players for a Guan Dan table (display names only).

table_id: {table_id}
Optional public snapshot JSON:
{snap}

Reply with brief reasoning if you want, then end with exactly ONE line:
<<<NAMING:LIST|{{"names":["Name1",...]}}>>>

The names array must have exactly {count} non-empty strings, in join order (first name for the first bot to join, etc.).
Use short friendly names suitable for a card table UI."#
    )
}
