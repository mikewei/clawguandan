---
name: guandan
slug: guandan
description: >-
  Play GuanDan(掼蛋) via clawguandan CLI as an AI player. Use when users ask to plan GuanDan or create/list/join tables in game.
version: 0.1.0-beta.7
author: mikewei
license: MIT
metadata:
  hermes:
    tags: [Game, AI]
    homepage: https://github.com/mikewei/clawguandan
prerequisites:
  commands: [npm]
---

# Guandan

You can play **GuanDan** (掼蛋) card game through the `clawguandan CLI` as one or more AI players.

## Prerequisites

1) Check whether CLI is already available:
   ```
   ./scripts/run.sh show version
   ```
   If not available, install it (only if you trust the package source):
   ```
   npm install @mikewei-labs/clawguandan@0.1.0-beta.7
   ```

2) Check server ready
   Run:
   ```
   ./scripts/run.sh server status
   ```
   If the `status` is unreachable, you can restart the local server:
   ```
   ./scripts/run.sh server restart
   ```
   You can see the Web UI URLs for human users once everything is ready.

## Quick start (Bot Mode, default)

1) Read the current table list:
   ```
   ./scripts/run.sh table list
   ```

2) Confirm whether **you** should create the table or the user already specified a table.
   - If you create the table, run `./scripts/run.sh table create "<a_cool_table_name>"` and obtain `tableId`.

3) Confirm with the user how many **Bot players** should join, then run the command in background:
   ```
   ./scripts/run.sh bot llm-bot --default-script openclaw --players <number_of_bot_players> -t <tableId> -v
   ```
   If some error occurs, try to fix it and retry.

4) If it still does not work, confirm with user either degrade to rule-based bot or switch to `Subagent Mode`:
   - degrade to rule-based bot player:
     ```
     ./scripts/run.sh bot rule-bot --players <number_of_bot_players> -t <tableId> -v
     ```
   - switch to `Subagent Mode` (see [references/sugagent_mode.md]).

5) Game started. Report game status when needed.