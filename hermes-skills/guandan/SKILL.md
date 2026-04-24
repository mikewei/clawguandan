---
name: guandan
slug: guandan
description: >-
  Play GuanDan(掼蛋) via clawguandan CLI as an AI player. Use when users ask to plan GuanDan or create/list/join tables in game.
author: mikewei
license: MIT
metadata:
  hermes:
    tags: [Game, Poker]
    homepage: https://github.com/mikewei/clawguandan
prerequisites:
  commands: [clawguandan]
---

# Guandan

You can play **GuanDan** (掼蛋) poker game through the `clawguandan` CLI as one or more AI players.

## Hard rules

1) Interact **only** through the CLI; the command is `clawguandan`. Do not guess game state.
2) Treat **only** JSON returned by the CLI as the source of truth.
3) Decide quickly by default; slow down when the user asks for deeper thought.
4) When mentioning **IDs** (such as `tableId`, `playerId`), copy them **verbatim, character-by-character** from the original text. Do **not** paraphrase or guess ID formats.

## Available commands

```
clawguandan table create "<name>"
clawguandan table join -t <tableId> --name <playerName> --type bot --model <llmModelName>
clawguandan play ready -t <tableId> -p <playerId>
clawguandan play wait4myturn -t <tableId> -p <playerId>
clawguandan play playcards -t <tableId> -p <playerId> "<c1,c2,...>"
clawguandan play pass -t <tableId> -p <playerId>
clawguandan play tribute -t <tableId> -p <playerId> "<card>"
clawguandan play returncard -t <tableId> -p <playerId> "<card>"
clawguandan show rules
clawguandan show version
```

- Do not use commands other than those above; do not explore flags on your own, and do not use Web Search for command syntax.

## Game startup flow

1) Confirm table existence (either user provided `tableId` or you create one).
   - If you create the table, run `clawguandan table create "<name>"` and obtain `tableId`.
2) Confirm how many **AI players** to be joined (up to 3 allowed in one session due to the hermes subagent limit), for each AI player:
   - pick a short and **cool** **player name**.
   - run: `clawguandan table join -t <tableId> --name "<playerName>" --type bot --model "<llmModelName>"`, and capture the `playerId` in the output carefully.
   - **CRITICAL**: Do NOT spawn any subagents yet. 
   - Keep a list of `[ {playerName: "...", playerId: "..."} ]`.
3) Once ALL players have been joined and their `playerIds` are recorded, spawn **subagents** in batch mode:
    1. Construct a single `delegate_task` call using the `tasks` array.
    2. Each entry in the `tasks` array MUST be a separate object for one player, containing:
       - `goal`: "DON'T QUIT! REPEAT the 'Subagent execution flow' until COMPLETE the game!"
       - `context`: `"Your playerName:'<playerName>', playerId:'<playerId>', tableId:'<tableId>'. Read guandan skill and execute flow."`
       - `toolsets`: ["skills", "terminal"]
    3. Set `max_iterations` to 500.
    4. DO NOT set any other properties in the root parameter object.
4) Always follow the `Task management strategy` below.

### Task management strategy

1) Whenever the `delegate_task` complete, **immediately** spawn the subagents for all players again (use `delegate_task` tool in batch mode, with the parameters described above).
2) Only if you receive `[GAME OVER]` message or the user asks to end the game, tell the user the game result and provide a short summary.

## Subagent execution flow

1) **MUST Repeat** the following flow until hand game over:

   1. Run `clawguandan play wait4myturn -t <tableId> -p <playerId>` in the **foreground** with timeout of 600 secs. It may block for a while, be patient.
   2. Read the returned JSON; focus on:
      - `status` / `phase`
      - `expect.kind`
      - `expect.actorPlayerIds`
      - `expect.legalActions`
      - `private.handCards` (if present)
      - `hand.topPlay` (if present)
   3. If your `playerId` is **not** in `actorPlayerIds`, continue with `wait4myturn`.
   4. If `expect.kind` requires you to act, execute **one** action from `expect.legalActions` using the `Decision policy` below.
   5. Only if `status` is `finished` or if `phase` is `scoring` and you have played a whole hand, then quit and complete the subagent, otherwise go on.
      - On complete you MUST **explicitly** return `[GAME INPROGRESS]` message.

2) On any condition you are not sure, just run from the start of `Subagent execution flow`.

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
