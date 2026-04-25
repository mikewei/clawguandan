# Subagent Mode

## Hard rules

1) Interact **only** through the CLI; the command is `clawguandan`. Do not guess game state.
2) Treat **only** JSON returned by the CLI as the source of truth.
3) Decide quickly by default; slow down when the user asks for deeper thought.
4) When mentioning **IDs** (such as `tableId`, `playerId`), copy them **verbatim, character-by-character** from the original text. Do **not** paraphrase or guess ID formats.

## Game startup flow

1) Delete the `### guandan pending ###` subsection in `TOOLS.md` (if present) and ignore its content.
2) Confirm whether **you** create the table or a given `tableId` is available.
   - If you create the table, run `clawguandan table create "<name>"` and obtain `tableId`.
   - If a table id was given, go to the next step.
3) Confirm with the user how many **AI players** should join; pick a short and **cool** **player name** for each.
4) For each player, join: `clawguandan table join -t <tableId> --name "<playerName>" --type bot --model "<llmModelName>"`, and read the `playerId` in the output carefully.
5) For each player, spawn a **Subagent** task via **`sessions_spawn`** (**run** mode, with **Label = playerName**). Build a **complete** task prompt that **must** include:
   - An **explicit** prompt whose text includes: `Ignore all previous game history, read Guandan Skill, and execute "Subagent execution flow" from start`.
   - Key fields such as `tableId`, `playerId`, etc.: values must be **exact**; wrap each value in **double quotes** (e.g. `"..."`).
   - An **explicit** prompt whose text includes: `Do not casually invoke "sessions_yield"`.
6) Update `TOOLS.md` by writing a new `### guandan pending ###` subsection. It **must** include:
   - An **explicit** prompt whose text includes: `Read Guandan Skill, and follow the "Task management strategy"`.
   - `tableId`, and for each player: `playerId`, `playerName`, `Subagent` — each value **exact**, wrapped in **double quotes**.
7) Always follow the `Task management strategy` below.

### Task management strategy

1) Whenever you receive a Subagent completion message, if it does **not** include `[GAME OVER]`, **immediately** spawn a **new** Subagent for that player via **`sessions_spawn`**. Build a complete task prompt that includes:
   - An **explicit** prompt whose text includes: `Ignore all previous game history, read Guandan Skill, and execute "Subagent execution flow" from start`.
   - Key parameters such as `tableId`, `playerId`, etc. — **exact** literals, each wrapped in **double quotes**.
   - The summary returned by the just-finished Subagent.
   - An **explicit** prompt whose text includes: `Do not casually invoke "sessions_yield"`.
2) If the user asks for game status, or if game issues occur:
   - Proactively check the status of all Subagents.
   - For exited or stuck Subagents, run the same respawn action described above.
3) If you receive `[GAME OVER]`, or the user asks to end the game:
   - Tell the user the game result and provide a short summary.
   - Delete the `### guandan status ###` subsection in `TOOLS.md` (if present).

## Game recovery flow

1) Try to read key game parameters from the `### guandan pending ###` subsection in context (or `TOOLS.md`).
2) Confirm the `tableId`, `playerId`, etc. to recover; ask the user if anything is missing.
3) Follow the `Task management strategy` to verify and restore Subagent-related state.

## Subagent execution flow

1) **Repeat** the following flow until game over:

   1. Run: `play wait4myturn -t <tableId> -p <playerId>`. It may block for a while, be patient and use timeout of 60000 when using process poll/log.
   2. Read the returned JSON; focus on:
      - `status` / `phase`
      - `expect.kind`
      - `expect.actorPlayerIds`
      - `expect.legalActions`
      - `private.handCards` (if present)
      - `hand.topPlay` (if present)
   3. Only if `status` is `finished` (that means game over), then quit from the loop, otherwise go on.
   4. If your `playerId` is **not** in `actorPlayerIds`, continue with `wait4myturn`.
   5. If `expect.kind` requires you to act, execute **one** action from `expect.legalActions` using the `Decision policy` below.

2) When you finish via **`sessions_yield`**, return results **explicitly**:
   - If the game is **not** over, you **must** return explicitly: `<playerId>: Guandan game in progress; please sessions_spawn another Subagent for me`
   - If the game is over, you **must** return explicitly: `[GAME OVER]`

### Decision policy

Decide based on `expect.kind`:

1) **`ready`**: `clawguandan play ready -t <tableId> -p <playerId>`
2) **`tribute`**: `clawguandan play tribute -t <tableId> -p <playerId> "<card>"`
   - Tribute the **highest-ranked single card** you can, and avoid spending **heart suit level cards** (wild cards) and other **critical wild** material when possible.
3) **`exchange`** (return after tribute): `clawguandan play returncard -t <tableId> -p <playerId> "<card>"`
   - Return the **lowest-value** unwanted single card, **different** from the tribute card you received.
4) **`play`**: either `clawguandan play playcards -t <tableId> -p <playerId> "<c1,c2,...>"` or `clawguandan play pass -t <tableId> -p <playerId>`
   - If you are leading a new trick (empty `topPlay`), shed more weak cards to shrink your hand count:
     - prefer the small (see `Beating rules` below) legal non-bomb combinations;
     - among similarly low-strength options, prefer combinations that use more cards
   - If you must BEAT `topPlay`, make sure your play can BEAT it according to the `Beating rules`.
     1. First find the **smallest same pattern** combination that still beats it;
     2. If nothing of the same type beats it, consider the **smallest bomb** (Do NOT break bombs) that does;
     3. If a bomb is too costly and the situation is not critical, **`pass`**.
   - When legality is uncertain, prefer **`pass`**.

### Partnership style

- Goal: **both players on your partnership get out quickly**, not only racing to be first yourself.
- When your **partner** (across from you) is clearly strong, take the lead less often and spend fewer bombs.
- Do not spend high-value resources (4-jokers bomb, large bomb, critical wildcards) on non-critical tricks.
  - Use bombs mainly to intercept opponents who are about to go out.
- Breaking a larger pattern into smaller ones (e.g. splitting a `pair` into two `single`s, or a `plate` into two `triple`s) **weakens future strength**; only do this when necessary (e.g. no better legal follow/beat, strong pressure, or clear endgame need).
  - Do NOT break bombs into weaker-bomb or non-bomb cards.

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

### Complete rules

Run `clawguandan show rules` to see the complete game rules if you really need it.

### Error recovery

- On playcards errors: read the `Beating rules` first
- On other action failures: resume with `play wait4myturn -t <tableId> -p <playerId>`, then recompute the action.