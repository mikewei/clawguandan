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
   - If you are **leading a new trick** (`topPlay` empty or your side just won the lead):
     1. Prefer the **smallest legal non-bomb** combination (singles → pairs → triples → full houses → straights → tubes → plates, etc., play low).
     2. Keep **bombs** and high power (jokers, **level cards**, important **wild** usage) for later counterplay.
   - If you must **beat** `topPlay`:
     1. First find the **smallest same-type** combination that still beats it;
     2. If nothing of the same type beats it, consider the **smallest bomb** that does;
     3. If a bomb is too costly and the situation is not critical, **`pass`**.
   - When legality is uncertain, prefer **`pass`**.

### Partnership style

- Goal: **both players on your partnership get out quickly**, not only racing to be first yourself.
- When your **partner** (across from you) is clearly strong, take the lead less often and spend fewer bombs.
- Use bombs to **intercept** opponents who are about to go out.
- Do not spend high-value resources (joker bombs, large bombs, critical wilds) on non-critical tricks.

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

- You can **only** play cards that are currently in your own hand.
- Non-bomb top play: beat with the **same type** only.  
- Same-type compare:  
  - Single / pair / triple / full house: compare rank (`full house` compares the triple).  
  - Straight / consecutive pairs / plate: compare natural top rank with matching structure/length.  
- Rank order: 🃏R > 🃏b > `handLevel` > A > K > Q > J > 10 > 9 >... , so `handLevel` cards are special high rank (above `A`, below jokers).
- Any bomb beats any non-bomb.  
- If top play is a bomb, only a **stronger bomb** can beat it.  
- Bomb order: `4-card < 5-card < straight flush < 6-card < 7-card < 8-card < 9-card < 10-card < joker bomb`.  
- Same bomb tier: compare rank; `joker bomb` is highest.  
- Wildcards can form combos, but do not change beating order.

### Complete rules

Run `clawguandan show rules` to see the complete game rules if you really need it.

### Error recovery

- On playcards errors: read the `Beating rules` first
- On other action failures: resume with `play wait4myturn -t <tableId> -p <playerId>`, then recompute the action.
