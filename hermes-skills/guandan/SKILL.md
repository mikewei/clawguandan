---
name: guandan
slug: guandan
description: >-
  Play GuanDan(掼蛋) via clawguandan CLI as AI players. Use when users ask to play GuanDan or create/list/join tables in game.
version: 0.1.1
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

You can play **GuanDan** (掼蛋) card game through the `clawguandan` CLI as one or more AI players.

## Prerequisites

1) Ensure the `clawguandan` command exists (via `clawguandan -V`).
   If not found, install it (only if you trust the package source):
   ```
   npm install -g @mikewei-labs/clawguandan@latest
   ```

2) Verify server readiness:
   ```
   clawguandan server status
   ```
   If the `status` is unreachable, you can restart the local server:
   ```
   clawguandan server restart
   ```
   You can see the Web UI URLs for human users once everything is ready.

## Quick start (Bot Mode, default)

1) Read the current table list:
   ```
   clawguandan table list
   ```

2) Confirm whether **you** should create the table or the user already specified a table.
   - If you create the table, run `clawguandan table create "<a_cool_table_name>"` and obtain `tableId`.

3) Confirm with the user how many **Bot players** should join, then run the command in background:
   ```
   clawguandan bot llm-bot --default-script hermes --players <number_of_bot_players> -t <tableId> -v
   ```
   If some error occurs, try to fix it and retry.

4) If it still does not work, confirm with user either degrade to rule-based bot or switch to `Subagent Mode`:
   - degrade to rule-based bot player:
     ```
     clawguandan bot rule-bot --players <number_of_bot_players> -t <tableId> -v
     ```
   - (Only if the user has confirmed) switch to `Subagent Mode` (load `skill_view("guandan", "references/subagent_mode.md")`).

5) Game started. Report game status when needed.
