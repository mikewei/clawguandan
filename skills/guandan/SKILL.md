---
name: guandan
slug: guandan
description: >-
  Play GuanDan(掼蛋) via clawguandan CLI as an AI player. Use when users ask to plan GuanDan or create/list/join tables in game.
metadata:
  openclaw:
    emoji: "🃏"
    requires:
      bins: []
    os:
      - linux
      - darwin
      - win32
---

# Guandan

You are an AI player for **GuanDan** (掼蛋) through the `clawguandan` CLI.

## Hard rules

1) Interact **only** through the CLI; the command is `clawguandan`. Do not guess game state.
2) Treat **only** JSON returned by the CLI as the source of truth.
3) Decide quickly by default; slow down when the user asks for deeper thought.

## Available commands

```
clawguandan table create "<name>"
clawguandan table join -t <tableId> --name <playerName> --type ai --model <llmModelName>
clawguandan play ready -t <tableId> -p <playerId>
clawguandan play wait4myturn -t <tableId> -p <playerId>
clawguandan play playcards -t <tableId> -p <playerId> "<c1,c2,...>"
clawguandan play pass -t <tableId> -p <playerId>
clawguandan play tribute -t <tableId> -p <playerId> "<card>"
clawguandan play returncard -t <tableId> -p <playerId> "<card>"
```

- Run all commands in the foreground; do not background them.
- Do not use commands other than those above. Do not invent CLI flags, and do not use Web Search for command syntax.

## Game startup flow

1) Delete the `### guandan status ###` subsection in `TOOLS.md` (if present) and ignore its content.
2) Confirm whether **you** create the table or the user already gives you a **table id** (`tableId`).
   - If you create the table, run `clawguandan table create "<name>"` and obtain `tableId`.
   - If a table id was given, go to the next step.
3) Confirm how many **players** should join; pick a short, **fun** **player name** for each.
4) For each player, join: `clawguandan table join -t <tableId> --name <playerName> --type ai --model <llmModelName>`
5) For each player, spawn a **Subagent** task via **`sessions_spawn`** (**run** mode). Build a **complete** task prompt that **must** include:
   - An **explicit** prompt whose text includes: `"Read Guandan Skill, and execute Subagent execution flow"`.
   - Key parameters such as `tableId`, `playerId`, etc.
   - An **explicit** prompt whose text includes: `"Do not call sessions_yield casually"`.
6) Update `TOOLS.md` by writing a new `### guandan pending ###` subsection. It **must** include:
   - An **explicit** prompt whose text includes: `"Read Guandan Skill, and execute task management flow"`.
   - `tableId`, and for each player: `playerId`, `playerName`, `Subagent`.
7) Enter the task management flow.

### Task management flow

1) Keep looping until you receive clear confirmation that the game has ended:
   - If any Subagent **completes**, **immediately** spawn a **new** Subagent for that player via **`sessions_spawn`**.
   - If any Subagent appears **stuck**, **immediately** spawn a **new** Subagent for that player via **`sessions_spawn`**.
2) Tell the user the game result and provide a short summary.
3) Delete the `### guandan status ###` subsection in `TOOLS.md` (if present).

## Subagent execution flow

**Repeat** the following flow until the game ends (**status** is `Finished`) **or** you estimate the **context window** is nearly full (**>60%**):

1) Run: `play wait4myturn -t <tableId> -p <playerId>`
2) Read the returned JSON; focus on:
   - `status` / `phase`
   - `expect.kind`
   - `expect.actorPlayerIds`
   - `expect.legalActions`
   - `private.handCards` (if present)
   - `hand.topPlay` (if present)
3) If your `playerId` is **not** in `actorPlayerIds`, continue with `wait4myturn`.
4) If `expect.kind` requires you to act, execute **one** action from `expect.legalActions` using the policy below.

If the Subagent exits, return the **exit reason**. When the game has **clearly** ended, return **`[GAME OVER]`**.

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

### Error recovery

- On action failure **1–2** times: resume with `play wait4myturn -t <tableId> -p <playerId>`, then recompute the action.
- On **≥3** consecutive failures: print a short state summary plus the last command, stop blind retries, and ask the user how to proceed.
