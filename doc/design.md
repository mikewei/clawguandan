# Design Doc

## Overview

This document defines the backend domain model and HTTP interfaces for a 4-player, 2-team Guan Dan game server.
The design prioritizes:

- clear separation between game rules and transport/API concerns;
- deterministic rule validation and reproducible game transitions;
- a **monotonic sequence number (`seq`) per table** so clients (including AI agents) can follow one primary stream of updates;
- auditable actions and deterministic conflict handling suitable for multiplayer environments.

## Architecture

### Core Entities

- `Table`
  - Responsibility: lifecycle container for one Guan Dan session.
  - Key fields: `tableId`, `status` (`waiting`, `in_game`, `finished`), `seats`, `currentHandId`, `seq` (current), `createdAt`, `updatedAt`.
  - Rules: exactly 4 occupied seats before first hand can start.
- `StateTransition` (table-scoped)
  - Responsibility: immutable record of one committed state change for client reconstruction and replay.
  - Key fields: `seq` (strictly increasing from `0` at table creation until `finished`), `tableId`, `delta`, `timestamp`, optional `causalActionId`.
  - Storage: **in-memory ring buffer** (or bounded log) per table; optional persistence is out of scope unless durability is required.
- `Seat`
  - Responsibility: fixed logical position in one table (`E`, `S`, `W`, `N`) and stable turn order.
  - Key fields: `position`, `playerId`, `isReady`.
  - Rules: partnerships are fixed by opposite seats (`E-W`, `S-N`).
- `Player`
  - Responsibility: identity and per-table participation state.
  - Key fields: `playerId`, `displayName`, `connected`, `lastSeenAt`.
  - Notes: long-term profile data should stay outside game domain.
- `Team`
  - Responsibility: level/progression owner and winner/loser role assignment between hands.
  - Key fields: `teamId`, `seatA`, `seatB`, `level`, `aceFailedAttempts`.
  - Rules: both teams start at level `2`; special A-level rules are enforced in scoring.
- `Hand`
  - Responsibility: one fully dealt and scored round.
  - Key fields: `handId`, `tableId`, `handLevel`, `dealerSeat`, `leaderSeat`, `stage`, `tributePlan`, `finishingOrder`, `winnerTeamId`, `startedAt`, `endedAt`.
  - `stage` values: `dealing`, `tribute`, `exchange`, `playing`, `scoring`, `completed`.
- `Card`
  - Responsibility: normalized card symbol unit in a double deck rules context.
  - Key fields: `symbol`, `rank`, `suit`.
  - Rules: 108 cards total; include two red jokers and two black jokers.
- `CardView` (derived)
  - Responsibility: contextual interpretation under current hand level.
  - Derived properties: `isLevelCard`, `isWildCard` (heart + level rank), `levelOrderRank`, `naturalOrderRank`.
  - Notes: do not persist; compute from `Card` + `handLevel`.
- `PlayerHand`
  - Responsibility: hidden private cards for one seat in one hand.
  - Key fields: `handId`, `seat`, `cards`, `remainingCount`.
  - Rules: only owner and trusted server logic can read full card list.
- `Trick`
  - Responsibility: one continuous contest until three consecutive passes.
  - Key fields: `trickId`, `handId`, `leadSeat`, `currentTopPlay`, `consecutivePasses`, `status`.
- `PlayAction`
  - Responsibility: append-only event of `play` or `pass`.
  - Key fields: `actionId`, `handId`, `trickId`, `seat`, `actionType`, `cards`,
  `declaredWildMapping`, `combinationType`, `createdAt`.
  - Rules: every legal transition must be recorded as an action event.
- `Combination`
  - Responsibility: canonical result of rule engine parsing/validation.
  - Types:
    - Ordinary: `single`, `pair`, `triple`, `full_house`, `straight`, `tube`, `plate`
    - Bomb: `quadruple`, `quintuple`, `straight_flush`, `sextuple`, `septuple`,
    `octuple`, `nonuple`, `decuple`, `four_joker`
  - Comparable key: `(class, bombTier, primaryRankByRuleOrder, tiebreakers...)`.
- `HandResult`
  - Responsibility: scoring output and next-hand setup inputs.
  - Key fields: `finishingOrder`, `winType` (`1-2`, `1-3`, `1-4`), `promotionDelta`,
  `nextDeclarerTeamId`, `nextLeaderPolicy`, `specialEvents` (A-level demotion, tribute cancellation).

### Card Symbol Notation

To keep APIs and prompts compact and human-readable, cards use a unified symbol format:

- Standard card format: `<suit><rank>`
  - Suits: `♠` (spades), `♥` (hearts), `♦` (diamonds), `♣` (clubs)
  - Ranks: `A, K, Q, J, 10, 9, 8, 7, 6, 5, 4, 3, 2`

Examples:

- `♥A` = Ace of hearts
- `♠3` = 3 of spades
- `♦10` = 10 of diamonds
- `♣K` = King of clubs

Jokers:

- `🃏R` = red joker
- `🃏b` = black joker

Notes for double-deck Guan Dan:

- Symbol notation is for display and prompts.
- API and game-state operations use symbols only. Duplicate copies are represented by multiplicity in a multiset.

### Internal Hand Representation (Server)

Server stores each hand as a symbol multiset (no `cardId`), indexed by a fixed symbol table of 54 card symbols.

- Suggested storage per symbol: `uint8` bit-pattern in `{0x0, 0x1, 0x3}`
  - `0x0`: none of the two copies exists
  - `0x1`: first copy exists
  - `0x3`: first and second copies exist
  - `0x2`: invalid/reserved and MUST NOT appear
- Containment check for one symbol slot uses bitwise operation: `contains(A, B) <=> (A & B) == B`
  - Here `A` is the player's hand slot, `B` is requested-play slot.
- Slot transitions:
  - add one copy: `0x0 -> 0x1`, `0x1 -> 0x3`, `0x3 -> error`
  - remove one copy: `0x3 -> 0x1`, `0x1 -> 0x0`, `0x0 -> error`
- Whole-play legality first checks multiset containment per slot, then runs combination/wildcard/rank validation.

### Game Logic

The game logic is implemented as a state machine.

#### 1) Table Lifecycle

1. `waiting`: table exists, players join/leave, seats can be assigned.
2. `in_game`: at least one hand has started; table accepts hand actions only.
3. `finished`: game-over condition reached (A-level declarer wins by `1-2` or `1-3`).

#### 2) Hand State Machine

1. `dealing`
  - Build and shuffle 108-card deck.
  - Determine draw/deal policy using previous hand result.
  - Distribute 27 cards to each seat.
  - Resolve initial leader candidate (first hand: drawn face-up card rule; later hands: tribute outcome).
2. `tribute`
  - Determine tribute payer(s) from previous hand win type.
  - Evaluate cancellation rules (red jokers).
  - If canceled, skip to `playing` with fallback leader rule.
3. `exchange`
  - Collect tribute cards and mandatory return cards.
  - Enforce constraints:
    - tribute must be highest single non-wild card;
    - exchange card must be different from received tribute card.
4. `playing`
  - Maintain turn pointer in counterclockwise order.
  - On each turn: accept `play` or `pass`.
  - Validate played cards:
    - combination shape legality;
    - wild-card declaration completeness;
    - beat rule against current top play (same type higher, or bomb override, or higher bomb).
  - Trick ends after 3 consecutive passes; last successful player leads next trick.
  - Hand ends when one team has both players empty.
5. `scoring`
  - Compute finishing order and win type (`1-2`, `1-3`, `1-4`).
  - Apply level promotion and A-level special rules.
  - Determine next hand declarers, shuffler, and opener policy.
6. `completed`
  - Persist immutable summary and emit game events.

#### 3) Rule Engine Design

- `CardOrderService`
  - Produces natural order and level order comparators.
  - Handles level-card promotion and ace low/high behavior in sequences.
- `WildcardResolver`
  - Validates and normalizes `declaredWildMapping`.
  - Rejects joker substitution and illegal multi-target mappings.
- `CombinationParser`
  - Parses selected cards into a canonical `Combination`.
  - Ensures sequence constraints:
    - straights/tubes/plates use natural order rank progression;
    - no invalid A-wrap interior patterns.
- `BeatComparator`
  - Compares candidate play with top play under current context.
  - Enforces bomb hierarchy and same-type ranking semantics.
- `ScoringService`
  - Computes promotion deltas (`1`, `2`, `4`) by win type.
  - Applies A-level terminal, stay, demote, and three-failed-attempt rules.
  - Applies special ace-finish demotion trigger.

#### 4) Consistency and Safety

- Server-authoritative state: clients never decide legality.
- **Primary concurrency key is table `seq`**: each successful mutation advances `seq` by exactly one and appends one `StateTransition`.
- Action requests SHOULD include `seq` equal to the server’s **current** head (the client’s `lastAppliedSeq` after it has applied all transitions). If it does not match, respond with `409 Conflict` and a body that includes the authoritative `currentSeq` (and optionally a pointer for catch-up via `nextstate` or `snapshot`).
- `idempotencyKey` is intentionally omitted: with strict `seq` optimistic locking, only one action can be accepted for a given table head.
- The append-only **game action log** (plays, passes, tribute, etc.) remains the source of truth for rules; `StateTransition.delta` may embed or reference those events for agents.

### Web API

The API is **not** required to be strictly RESTful. It is optimized for **human clients, bots, and AI agents**: one long-polling stream for “what changed next,” plus a small set of **action** endpoints.

Base path: `/api/v1`

#### Sequence model (`seq`)

- For each `tableId`, `seq` is an integer starting at `0` when the table is created.
- Every committed state transition (join, ready, deal, tribute step, play, pass, scoring tick, etc.) increments `seq` by `1`.
- The server keeps an in-memory ordered log of `{ seq, transitionType, delta, ... }` per table so a client can rebuild state by applying transitions from `1..N` (or by applying a snapshot + tail).
- **Catch-up rule**: if a client’s `sinceSeq` is behind (server `currentSeq > sinceSeq`), `nextstate` MUST return transition `sinceSeq + 1` immediately without waiting.

#### Canonical state schema

`TableState` is the materialized state at a specific `seq`.

```json
{
  "tableId": "t_123",
  "seq": 42,
  "status": "waiting|in_game|finished",
  "phase": "table_setup|dealing|tribute|exchange|playing|scoring|completed",
  "seats": {
    "E": { "playerId": "p1", "playerName": "Alice", "playerType": "human", "ready": true, "remainingCount": 12 },
    "S": { "playerId": "p2", "playerName": "Bot-S", "playerType": "ai", "ready": true, "remainingCount": 8 },
    "W": { "playerId": "p3", "playerName": "Bob", "playerType": "unknown", "ready": true, "remainingCount": 16 },
    "N": { "playerId": null, "playerName": null, "playerType": null, "ready": false, "remainingCount": null }
  },
  "teams": [
    { "teamId": "team_ew", "seats": ["E", "W"], "level": "8", "aceFailedAttempts": 1, "role": "declarer|opponent" },
    { "teamId": "team_sn", "seats": ["S", "N"], "level": "6", "aceFailedAttempts": 0, "role": "declarer|opponent" }
  ],
  "hand": {
    "handId": "h_9",
    "handIndex": 9,
    "handLevel": "8",
    "leaderSeat": "S",
    "turnSeat": "W",
    "trickIndex": 4,
    "history": [
      {
        "seq": 39,
        "actionId": "a_1201",
        "seat": "S",
        "actionType": "play",
        "combinationType": "pair",
        "cards": ["♠A", "♠A"],
        "declaredWildMapping": {},
        "timestamp": "2026-03-31T12:30:00Z"
      },
      {
        "seq": 40,
        "actionId": "a_1202",
        "seat": "W",
        "actionType": "pass",
        "timestamp": "2026-03-31T12:30:05Z"
      }
    ],
    "topPlay": {
      "seat": "S",
      "combinationType": "pair",
      "cards": ["♠A", "♠A"],
      "declaredWildMapping": {}
    }
  },
  "expect": {
    "kind": "wait|join|ready|tribute|exchange|play|pass|game_over",
    "actorPlayerIds": ["p3"],
    "legalActions": ["play", "pass"],
    "deadlineAt": null
  },
  "scoreboard": {
    "finishingOrder": [],
    "lastHandResult": null,
    "gameWinnerTeamId": null
  }
}
```

Visibility:

- Player/observer always receives public fields in `TableState`.
- If request has a valid `playerId`, server MAY include:

```json
{
  "private": {
    "playerId": "p3",
    "seat": "W",
    "handCards": ["♠A", "♠A", "♥10", "🃏R"],
    "playHints": { "canPlay": true, "canPass": true }
  }
}
```

- Observer request (no `playerId`) MUST NOT include `private`.

`hand.history` contract:

- `TableState.hand.history` is the full public action history of the **current hand**.
- Items are ordered by ascending `seq`; each item MUST be uniquely identified by `seq` (and optionally `actionId`).
- On `HAND_STARTED`, `hand.history` is reset to an empty list for the new `handId`.
- `hand.history` is authoritative for current-hand replay; table-level transition log remains authoritative for global `seq` continuity and recovery.

#### Delta schema (`StateTransition.delta`)

Each `seq` corresponds to exactly one transition envelope:

```json
{
  "seq": 43,
  "prevSeq": 42,
  "tableId": "t_123",
  "timestamp": "2026-03-31T12:34:56Z",
  "type": "ACTION_APPLIED",
  "delta": {
    "ops": [
      { "op": "replace", "path": "/hand/turnSeat", "value": "N" },
      { "op": "replace", "path": "/seats/W/remainingCount", "value": 15 },
      { "op": "add", "path": "/hand/history/-", "value": { "seq": 43, "actionId": "a_999", "seat": "W", "actionType": "play", "combinationType": "single", "cards": ["♣8"], "declaredWildMapping": {}, "timestamp": "2026-03-31T12:34:56Z" } },
      { "op": "replace", "path": "/hand/topPlay", "value": { "seat": "W", "combinationType": "single", "cards": ["♣8"], "declaredWildMapping": {} } }
    ],
    "privateOps": [
      { "targetPlayerId": "p3", "op": "replace", "path": "/private/handCards", "value": ["♠A", "♥10", "🃏R"] }
    ],
    "event": {
      "actionId": "a_999",
      "trigger": { "actionType": "play", "actorPlayerId": "p3" },
      "derived": ["TRICK_ENDED", "HAND_SCORED"]
    }
  }
}
```

`delta.ops` rules:

- `op` supports `add | replace | remove`.
- `path` uses JSON Pointer against canonical `TableState`.
- Ops are applied in order; all ops succeed or transition is invalid.
- `privateOps` are delivered only to matching `targetPlayerId`.
- `nextstate` returns only one latest transition (`sinceSeq + 1`), so history growth is performed incrementally by applying `add /hand/history/-` operations.
- Rejected action attempts are not represented as transitions and therefore do not appear in `nextstate` or `hand.history`.
- For accepted user actions, server SHOULD publish a single atomic transition (`type=ACTION_APPLIED`) even if multiple internal outcomes occur (for example trick end and hand score in the same step).

Recommended `type` values:

- Lifecycle: `TABLE_CREATED`, `PLAYER_JOINED`, `PLAYER_READY_CHANGED`, `GAME_STARTED`, `GAME_FINISHED`
- Hand flow: `HAND_STARTED`, `TRIBUTE_REQUIRED`, `TRIBUTE_SUBMITTED`, `EXCHANGE_SUBMITTED`
- Action transitions: `ACTION_APPLIED`
- Internal derived outcomes (inside `delta.event.derived`, not top-level `type`): `TRICK_ENDED`, `HAND_SCORED`

#### Core loop: `nextstate` (long polling)

This is the main way clients stay in sync and learn what to do next.

- `GET /tables/{tableId}/nextstate`
  - Query parameters (minimum):
    - `sinceSeq` (integer): client’s last applied sequence (use `0` before any transition is applied).
    - `playerId` (optional string): identifies the subscriber for per-player fields (`private`, personalized `prompt`, and action expectation).
    - `playerKey` (optional string): required when `playerId` is present; used for player identity validation.
      - When omitted, the caller is treated as an observer and receives public transitions only.
    - `timeoutMs` (optional): server waits up to this long for `seq` to advance past `sinceSeq`; on timeout, return `204 No Content` (see headers below).
  - Behavior:
    - If `sinceSeq < currentSeq`: return transition `sinceSeq + 1` immediately.
    - If `sinceSeq == currentSeq`: block until `currentSeq` becomes `sinceSeq + 1`, then return that single transition.
    - If `sinceSeq > currentSeq`: `400 Bad Request`.
  - Response body:

```json
{
  "seq": 42,
  "prevSeq": 41,
  "tableId": "t_123",
  "timestamp": "2026-03-31T12:34:56Z",
  "type": "ACTION_APPLIED",
  "lag": 0,
  "delta": {
    "ops": [],
    "privateOps": [],
    "event": {
      "trigger": { "actionType": "pass", "actorPlayerId": "p3" },
      "derived": ["TRICK_ENDED"]
    }
  },
  "expect": {
    "kind": "wait|join|ready|tribute|exchange|play|pass|game_over",
    "actorPlayerIds": ["p3"],
    "legalActions": ["play", "pass"],
    "deadlineAt": null
  },
  "prompt": "It is your turn. Your current hand is: ♠A, ♥10, ♦10, ♣10, 🃏R. Choose one legal action: play or pass."
}
```

- `lag` (integer): `currentServerSeq - seq` for this transition at response time. `0` means the client is caught up to the table head after applying this transition; larger values mean more transitions remain (catch-up mode).
- On `204 No Content` (long-poll timeout with `sinceSeq == currentSeq` and no new transition), the response includes headers `X-Table-Seq` (current table head `seq`) and `X-Table-Lag: 0` so clients can confirm they are at the head without a JSON body.

- `delta` MUST be structured and machine-readable; `prompt` is the natural-language helper layer for human/AI clients.
- When `expect.actorPlayerIds` contains `playerId` and `expect.kind` requires a mutating action (`ready|tribute|exchange|play|pass`), `prompt` SHOULD include the acting player's current hand cards to improve agent success rate.
- Hand cards in `prompt` SHOULD use the card symbol notation (`♠♥♦♣`, `🃏R`, `🃏b`).
- Observer calls receive no `private` payload and should get read-only prompts.

Optional convenience:

- `GET /tables/{tableId}/snapshot`
  - **No query parameters:** returns the latest public `TableState` (flattened JSON with `seq`, `tableId`, `seats`, `expect`, etc.).
  - `playerId` (optional): adds a `private` object (hand cards, hints) for that seated player.
  - `playerKey` (optional): required when `playerId` is present.
  - `atSeq` (optional): in the current MVP, only the latest snapshot is supported; if `atSeq` is present and not equal to the current head `seq`, the server returns `400 Bad Request`.
  - Clients that apply `nextstate` deltas locally SHOULD call this once when they have no materialized `TableState` yet (first merge), because the first transition’s patch is not applicable to an empty document.

#### Table lifecycle and seat actions

- `POST /tables`
  - Body: optional table config (for example `{}`).
  - Response: `{ "tableId", "seq": 0, "status": "waiting" }`
- `POST /tables/{tableId}/join`
  - Body: `{ "playerType": "human|ai|unknown", "playerName": "...", "seat": "E|S|W|N|auto" }`
  - `playerType` is optional; when omitted, the server defaults it to `unknown`.
  - Response includes server-issued player identity and current head, for example:
    - `{ "playerId": "...", "playerKey": "<uuid>", "seat": "S", "playerType": "ai", "newSeq": 1 }`
  - On success: advances `seq`, append transition.
- `POST /tables/{tableId}/ready`
  - Body: `{ "playerId", "playerKey", "ready": true|false }` (no `seq`; the server applies against the current head).
  - Constraint: only a `playerId + playerKey` pair that matches an active seated player can call this endpoint.
  - **Idempotent:** if `ready` is already the requested value for that player, the server does not advance `seq` or append a transition; response is still `200` with `newSeq` equal to the current head.
  - When all 4 seated players become `ready=true`, the server automatically starts the first hand and emits the corresponding state transitions.

#### Gameplay actions (mutations)

All action endpoints:

- Accept `playerId` and `seq` (last known/current head).
- Accept `playerKey` together with `playerId` for identity validation.
- Constraint: only seated players with valid `playerId + playerKey` can call action endpoints; observers are read-only and cannot mutate game state.
- On success: response includes `newSeq` (equals previous `seq + 1`) and a minimal echo of applied action.
- Map game phases to concrete routes (names are indicative; keep them stable for agents):


| Phase (illustrative) | Endpoint                                     | Notes                                              |
| -------------------- | -------------------------------------------- | -------------------------------------------------- |
| Tribute              | `POST /tables/{tableId}/actions/tribute`     | `{ "card": "♠A" }`                                |
| Exchange             | `POST /tables/{tableId}/actions/return_card` | `{ "card": "♦10" }` (must differ from received tribute) |
| Play                 | `POST /tables/{tableId}/actions/play`        | `{ "cards", "declaredWildMapping?" }` (symbols only) |
| Pass                 | `POST /tables/{tableId}/actions/pass`        | `{}`                                               |

After a hand ends and enters the `scoring` phase, the server automatically starts the next hand (including tribute setup) once scoring has all required inputs; there is no explicit `next_hand` Web API.


Rule details (legality, wild declarations, bombs) are enforced only on the server; failed calls return `422` with a stable `error.code`.

Rejected action contract:

- If an action is rejected (`403`, `409`, `422`, etc.), server returns an error response and MUST NOT advance `seq`.
- Rejections MUST NOT produce a transition event in `nextstate`.
- Clients should recover by reading error payload (for example `currentSeq`) and continuing normal `nextstate` sync.

#### Agent-oriented client loop

High-level flow:

1. **Create or join** a table; remember `tableId`, `playerId`, and `playerKey`:
   - Point the CLI at a server: `clawguandan server use <hostOrIp[:port]>` or `clawguandan server new`.
   - **Create:** `clawguandan table create [<tableName>]` ; The command prints JSON; read and store `tableId`.
   - **Join:** `clawguandan table join -t <tableId> --name "<playerName>" --type ai [--seat E|S|W|N|auto]`.
2. **Ready:** `clawguandan play ready -t <tableId> -p <playerId>` ; CLI auto-loads `playerKey` from local auth session unless `-k/--player-key` is passed explicitly.
3. **Until the game is over**, repeat:
   - Run `clawguandan play wait4myturn -t <tableId> -p <playerId>` and **read stdout** (local full state / prompt materialized for this player).
   - **Think** the next legal move and run the matching `clawguandan play ...` command (for example `playcards`, `pass`, `tribute`, `returncard`).

Observer mode:

1. **Until the game is over**, repeat:
   - Run `clawguandan table sync -t <tableId> -p <playerId>` and **read stdout** (full state of the game).


#### Error model

- `400 Bad Request`: malformed payload or impossible `sinceSeq`.
- `404 Not Found`: unknown `tableId` / `playerId` in context.
- `403 Forbidden`: caller is not allowed to mutate state (for example, observer or non-seated `playerId` calling `ready`/action APIs).
- `409 Conflict`: stale `seq` on an action, seat conflict, or illegal lifecycle transition.
- `422 Unprocessable Entity`: rule violation (illegal combination, wrong turn, etc.).

Example:

`{ "error": { "code": "ILLEGAL_COMBINATION", "message": "...", "currentSeq": 17, "details": {} } }`

#### Design notes and open points

- **Durability**: in-memory `seq` log is enough for dev and single-node deployments; multi-instance or crash recovery needs persistence or external log—call out when scaling.
- **Long-poll proxies**: some HTTP proxies time out `GET`; allow `POST /tables/{tableId}/nextstate` with the same fields in JSON body if needed.
- **Prompt quality**: should be generated from structured `expect` + state, not hand-written per route, to avoid drift from rules.

### CLI

CLI commands are optional adapters over the Web API, for local testing and bot integration.

#### Command set

`server` commands

- `clawguandan server serve [--ip <ip>] [--port <port>]`
  - Run the server in the foreground (the same binary as the CLI).
- `clawguandan server new`
  - Start (spawn) a local server process in background (via `server serve`) and set current `use` target to this server.
  - If an existing local background server is already running, do not spawn another one; just switch `use` to the running server.
- `clawguandan server use <hostOrIp[:port]>`
  - Set the active server endpoint for all subsequent CLI calls.
- `clawguandan server status`
  - Print current active server and local profile summary.

`table` commands

- `clawguandan table list`
  - List tables on the active server with basic status summary.
- `clawguandan table create <tableName>`
  - Create a new table.
  - Example: `clawguandan table create "Friday Night #1"`
- `clawguandan table join -t <tableId> --name <playerName> [--type human|ai|unknown] [--seat E|S|W|N|auto] [--no-sync]`
  - Join a table and receive server-issued `playerId` + `playerKey`.
  - CLI writes `playerKey` to `<sessionDir>/auth.json` immediately (even with `--no-sync`).
  - By default, after a successful join the CLI runs an internal catch-up (`table sync`) and prints the **local full materialized state** (see `session.json` below). Use `--no-sync` to print only the join API JSON (for scripts).
  - `--type` defaults to `unknown`; `--seat` defaults to `auto`.
  - Example: `clawguandan table join -t t_100 --name "Bot-S" --type ai --seat auto`
- `clawguandan table nextstate -t <tableId> [-p <playerId>] [-k <playerKey>] [--seq <seq>]`
  - Long-poll for exactly one next transition (`sinceSeq + 1`).
  - Default stdout is the server JSON (includes `lag`).
  - With `-p` and **auto-seq**, updates `session.json`: on `200`, merges `delta` into the stored `TableState` and refreshes `private`; if there is no stored `TableState`, performs `GET snapshot` first.
  - Without `-p`, runs observer mode (public state only; no session updates).
- `clawguandan table sync -t <tableId> -p <playerId> [-k <playerKey>] [--timeout-ms ...]`
  - Repeats `nextstate` until `lag == 0` or `204` at head; then prints the **local full state** (flattened `TableState` plus optional `private` key). Does not support `--seq` (session auto-seq only).

`play` commands

- `clawguandan play ready -t <tableId> -p <playerId> [-k <playerKey>] [--no-sync]`
  - `POST .../ready` with `ready: true` (no `seq`). By default runs `table sync` afterward and prints local full state; `--no-sync` prints only the ready API response.
- `clawguandan play wait4myturn -t <tableId> -p <playerId> [-k <playerKey>] [--timeout-ms ...]`
  - If local session is already at head and `expect` says this player must act, exits immediately with local full state; otherwise long-polls `nextstate` until that is true. Auto-seq only (no `--seq`).
- `clawguandan play playcards -t <tableId> -p <playerId> [-k <playerKey>] [--seq <seq>] "<cards>"`
  - Submit a play action with symbol list, comma-separated.
  - Example: `clawguandan play playcards -t t_100 -p p_3 "♠A,♠A,♥10"`
- `clawguandan play pass -t <tableId> -p <playerId> [-k <playerKey>] [--seq <seq>]`
  - Submit a pass action for current turn.
- `clawguandan play tribute -t <tableId> -p <playerId> [-k <playerKey>] [--seq <seq>] "<card>"`
  - Submit tribute card symbol.
  - Example: `clawguandan play tribute -t t_100 -p p_3 "♠A"`
- `clawguandan play returncard -t <tableId> -p <playerId> [-k <playerKey>] [--seq <seq>] "<card>"`
  - Submit return/exchange card symbol after receiving tribute.
  - Example: `clawguandan play returncard -t t_100 -p p_1 "♦10"`
  - After a hand reaches `scoring`, there is no separate CLI command to start the next hand; the server advances automatically to the next hand when scoring is complete.

#### Seq modes

- Default mode is **auto-seq** for player commands with `-p <playerId>`.
- Auto-seq key: `<host>:<port>` derived prefix (e.g. `127.0.0.1_22222`), then `.<tableId>.<playerId>` (dot-separated), one **session directory** per key under `std::env::temp_dir()/clawguandan/<sessionKey>/session.json` (not in global `~/.config/clawguandan/config.json`).
- `session.json` stores at minimum `lastAppliedSeq`, and when using `table nextstate` / `table sync` / `wait4myturn`, also a materialized public `TableState` plus optional `privateView` for merging deltas and printing local full state.
- `auth.json` (same session directory) stores player authentication info (`playerId`, `playerKey`), decoupled from game state sync.
- Player key resolve priority: explicit `--player-key` > auto-load from `auth.json`.
- Since `playerKey` is server in-memory only, a server restart invalidates old keys; clients should re-join or provide a fresh key when receiving auth errors.
- `POST .../ready` does not participate in auto-seq for the request body; the CLI still uses session state for post-ready `table sync`.
- Observer mode (no `-p`) does not support auto-seq persistence.
- If `--seq` is explicitly provided on supported commands, the CLI runs in **manual-seq** mode: use that value for `sinceSeq` or action `seq`; do not read/write `session.json` for that command (`table sync` and `play wait4myturn` disallow `--seq`).

#### Runtime behavior

- Action commands submit intent only; they do not mutate local session state by themselves.
- After every **gameplay** action command returns, the client SHOULD call `table nextstate` or `table sync` for the same table/player.
- Local `TableState` / `lastAppliedSeq` are advanced from `GET snapshot` (baseline) and successful `nextstate` responses with `-p` (auto-seq), written to `session.json`.
- Action and ready success responses may include `newSeq`; the CLI does not treat them as authoritative for local materialized state—follow with `nextstate`/`sync` as above.

## Implementation Guidelines

The implementation should prioritize simplicity, quality, extensibility, and maintainability.

### Engineering principles

- Keep code concise, high-quality, and easy to evolve.
- Prefer clear module boundaries and low coupling.
- Add sufficient comments for non-obvious logic and tricky rule handling.
- Keep public interfaces stable and explicit; avoid hidden side effects.

### Language and runtime

- The main program is implemented in Rust.
- The codebase should maintain good memory and compute efficiency.
- Favor predictable allocation patterns and avoid unnecessary cloning in hot paths.

### Web framework

- Use `axum` as the web framework.
- Keep handler responsibilities thin:
  - parse and validate request;
  - delegate to domain services;
  - map domain result to API response.

### Observability and logging

- Use `tracing` for structured, leveled logs.
- Recommended log levels:
  - `error`: request failures, invariant violations, unrecoverable paths;
  - `warn`: recoverable but suspicious behavior (stale seq, retries, fallbacks);
  - `info`: lifecycle milestones (server start, table created, hand started/scored);
  - `debug`: detailed transitions and action traces for development;
  - `trace`: very fine-grained diagnostics (typically disabled by default).
- Include key context fields in spans/log events: `tableId`, `playerId`, `seq`, `actionType`, `transitionType`.

### Frontend

- Frontend page code uses plain JavaScript to keep implementation simple.
- Keep frontend logic focused on state rendering and command invocation.

### Packaging and deployment

- Static assets are compiled into the binary via `rust-embed`.
- Deployment target is a single binary artifact with no separate static-file bundle.

### Testing and quality gates

- Build comprehensive test cases for core rules, state transitions, API contracts, and CLI behaviors.
- Keep tests updated alongside development; when behavior changes, tests must be updated in the same change.
- All modifications must pass tests before they are considered complete.
