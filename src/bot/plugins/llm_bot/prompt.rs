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
- Do not spend high-value resources (joker bomb, large bomb, critical wildcards) on non-critical tricks.
  - Use bombs mainly to intercept opponents who are about to go out.
- Breaking a larger pattern into smaller ones (e.g. splitting a `pair` into two `single`s, or a `plate` into two `triple`s) **weakens future strength**; only do this when necessary (e.g. no better legal follow/beat, strong pressure, or clear endgame need).
  - Do NOT break bombs into weaker-bomb or non-bomb cards.
- If you are leading a new trick,
  - prefer the small (see `Beating rules` below) legal non-bomb combinations;
  - among similarly low-strength options, prefer combinations that use more cards

### Combination patterns (NOT beating order)
- `single`: 1 card
- `pair`: 2 cards of same rank
- `triple`: 3 cards of same rank
- `full house`: 3 cards of one rank + 2 cards of another rank
- `straight`: 5 consecutive ranks (non-flush)
- `consecutive pairs`: 3 consecutive pairs (6 cards total)
- `plate`: two consecutive triples (6 cards total)
- `bomb`: 4+ of a kind, straight flush, or joker bomb

### Beating rules
- Against a non-bomb top play, beat with the same pattern only.
  - Different non-bomb patterns (even with same rank) are NOT comparable.
- Same-pattern compare:
  - single / pair / triple / full house: compare rank (`full house` compares the triple rank).
  - straight / consecutive pairs / plate: compare natural top rank with matching structure and length.
- Rank compare:
  - Rank order (left beats right): 🃏R > 🃏b > `handLevel` > A > K > Q > J > 10 > 9 > 8 > 7 > 6 > 5 > 4 > 3 > 2
    - **NOTE**: `handLevel` (read from JSON) is the current level rank, that is big in order, and does not slot into A–K–…–2 by printed rank — only as `handLevel` in this order.
  - Same rank (even with different suits) are equal and can NOT beat each other.
- Any bomb beats any non-bomb.
- If top play is a bomb, only a stronger bomb can beat it.
- Bomb order (left beats right): 4-jokers bomb > 10-card > 9-card > 8-card > 7-card > 6-card > straight flush > 5-card > 4-card.
  - Do not split bombs when beating.
- Same bomb tier: compare rank; `4-jokers bomb` is highest.
- Wildcards can form combinations, but do not change beating order.
  - Wildcards must be used sparingly, only when necessary.

### Make your decision
Current step kind: {expect_kind}.
Table + private JSON (materialized):
{state_pretty}

Reply with analysis if you want, but do not cycle through the same alternatives in different words; pick one legal action and commit.

Output exactly ONE machine-readable line at the **end** of your reply, using this format (no other <<<...>>> markers):
<<<DECISION:KIND|payload>>>  or when there is no payload: <<<DECISION:KIND>>>

Where KIND is one of: READY, PASS, PLAY, TRIBUTE, RETURN-CARD.
- For READY, PASS: no payload; you may write <<<DECISION:PASS>>>.
- For PLAY, TRIBUTE, RETURN-CARD: include a single-line JSON payload after `|`, e.g. <<<DECISION:PLAY|{{"cards":["♠3","♥3"]}}>>>.
    * Note: copy the cards **character-by-character** from your `handCards` exactly (including the suits and ranks).

Choose a legal action consistent with expect.legalActions and your private hand when visible.
If you are leading a new trick (empty `topPlay`), shed more weak cards to shrink your hand count;
else if you choose PLAY, make sure your play can **BEAT** `topPlay.cards` according to the `Beating rules` above, otherwise choose PASS.
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
Use short friendly and cool names suitable for a card table UI."#
    )
}
