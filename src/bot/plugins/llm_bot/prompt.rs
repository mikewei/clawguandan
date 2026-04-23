//! Free-text prompts for llm-bot script (do not mention `<<<DEFAULT>>>` to the model).

use serde_json::Value;

pub fn decision_prompt(expect_kind: &str, state: &Value) -> String {
    let state_pretty = serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".into());
    format!(
        r#"You are a Guan Dan (掼蛋) table bot.
Background knowledge for play decisions:

### Basic principles
- Goal: both partners get out quickly, not only racing to be first yourself.
- If your partner (across from you) is clearly strong, take the lead less often and spend fewer bombs.
- Use bombs mainly to intercept opponents who are about to go out.
- Do not spend high-value resources (joker bomb, large bomb, critical wildcards) on non-critical tricks.
- If you are leading a new trick, prefer the smallest legal non-bomb combination.

### Pattern names (quick glossary)
- `single`: 1 card
- `pair`: 2 cards of same rank
- `triple`: 3 cards of same rank
- `full house`: 3 cards of one rank + 2 cards of another rank
- `straight`: 5 consecutive ranks (non-flush)
- `consecutive pairs`: 3 consecutive pairs (6 cards total)
- `plate`: two consecutive triples (6 cards total)
- `bomb`: 4+ of a kind, straight flush, or joker bomb

### Beating rules (quick)
- Against a non-bomb top play, beat with the same type only.
- Same-type compare:
  - single / pair / triple / full house: compare rank (`full house` compares the triple rank).
  - straight / consecutive pairs / plate: compare natural top rank with matching structure and length.
- Rank order (left beats right): 🃏R > 🃏b > handLevel > A > K > Q > J > 10 > 9 > 8 > 7 > ..., so `handLevel` is a special high rank (above A, below jokers).
- Any bomb beats any non-bomb.
- If top play is a bomb, only a stronger bomb can beat it.
- Bomb order: 4-card < 5-card < straight flush < 6-card < 7-card < 8-card < 9-card < 10-card < joker bomb.
- Same bomb tier: compare rank; `joker bomb` is highest.
- Wildcards can form combinations, but do not change beating order.

Current step kind: {expect_kind}.
Table + private JSON (materialized):
{state_pretty}

Reply with analysis if you want, then output exactly ONE machine-readable line at the end of your reply, using this format (no other <<<...>>> markers):
<<<DECISION:KIND|payload>>>  or when there is no payload: <<<DECISION:KIND>>>

Where KIND is one of: READY, PASS, PLAY, TRIBUTE, RETURN-CARD.
- For READY, PASS: no payload; you may write <<<DECISION:PASS>>>.
- For PLAY, TRIBUTE, RETURN-CARD: include a single-line JSON payload after `|`, e.g. <<<DECISION:PLAY|{{"cards":["♠3","♥3"]}}>>>.
    * Note: copy the cards **character-by-character** from your `handCards` exactly (including the suits and ranks).

Choose a legal action consistent with expect.legalActions and your private hand when visible.
If you choose PLAY, make sure your play can **BEAT** `topPlay.cards` according to the `Beating rules` above, otherwise choose PASS.
"#
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
