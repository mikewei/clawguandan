---
name: guandan
slug: guandan
description: >-
  Play GuanDan(掼蛋) card game via `clawguandan` CLI. Use when users ask to play GuanDan or create/list/join tables in game.
metadata:
  openclaw:
    emoji: "🃏"
    os:
      - linux
      - darwin
    requires:
      bins: ["npm"]
---

# Guandan

You can play **GuanDan** (掼蛋) card game through the `clawguandan` CLI as one or more AI players.

## Prerequisites

1) Check CLI installed
Run:
```
which clawguandan
```
If not found, install it if user trust it:
```
npm install -g @mikewei-labs/clawguandan@next
```

2) Check server ready
Run:
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
clawguandan bot llm-bot --default-script openclaw --players <number_of_bot_players> -t <tableId> -v
```
If some error occurs, try to fix it and retry.

4) If it still does not work, confirm with user either degrade to rule-based bot or switch to `Subagent Mode`:
  - degrade to rule-based bot player:
    ```
    clawguandan bot rule-bot --players <number_of_bot_players> -t <tableId> -v
    ```
  - switch to `Subagent Mode` (see [references/sugagent_mode.md]).
