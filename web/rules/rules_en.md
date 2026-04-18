# Guan Dan — concise rules

Short reference for **Guan Dan** (掼蛋). Full detail: see `doc/guandan_rules_en.md` in the repository.

## Overview

- **4 players**, two fixed partnerships (partners sit opposite). **108 cards** (double deck + four jokers: two red `🃏R`, two black `🃏b`).
- Play is **counterclockwise**. Goal: empty your hand and help your partner; the partnership that gets both players out first wins the **hand**. Finishing order sets **promotion** (levels).
- **13 levels** (ranks 2 … A). Both teams start at **2**. Game is won at level **A** under specific conditions (below).

## Level cards and wild cards

- The **hand level** (from the declarers’ team) sets **level cards**: cards of that rank have special strength ordering (**level order**): generally **above A and below black joker**, not at natural rank.
- **Heart suit level cards are wild**: each can stand for any non-joker card when forming combinations. Declarations are required when wilds are used ambiguously.
- **Natural order** (straights, tubes, plates, straight flushes): A high or low as allowed; jokers not in straights; level cards use **natural rank position** inside those shapes.
- **First hand** is always at level **2** (wild: both `♥2`).

## Deal (summary)

- First hand: random shuffle; left of shuffler cuts with one **face-up** card in the deck; draw anticlockwise from shuffler; 27 cards each; **face-up drawer leads** first trick.
- Later hands: **first from previous hand shuffles**; left cuts (no face-up); **last finisher draws first**; opener follows **tribute** rules below.

## Play and tricks

- A **trick** ends after **three consecutive passes**; last player to play leads the next trick.
- On ordinary combinations: follow with a **higher same-type** combination or any **bomb**; on a bomb, only a **higher bomb** (same bomb class or higher bomb tier) beats it. Passing does not block later plays in the same trick.
- Empty hand: always pass. If the player who should **lead** is out, **lead passes to partner**. Hand ends when **both** players of one partnership are out.
- **≤10 cards**: if asked, must state exact count.

## Ordinary combinations (7 types)

Ranked by **level order** where applicable (singles, pairs, triples, full houses by triple only).

1. **Single** — wild as single counts as level card strength.
2. **Pair** — two black or two red jokers ok; not mixed colors. Wild pairs count like level-card pairs.
3. **Triple** — no joker triple; max triple is three level cards (only bombs beat).
4. **Full house** — triple + pair; compared by **triple only**; equal triples cannot chain.
5. **Straight** — five cards, consecutive **natural** ranks, **not all one suit**; top card in natural order wins; `A-2-3-4-5` lowest straight by rank 5; no illegal A-wrap interiors; wilds allowed per rules.
6. **Tube** — three consecutive pairs in natural order; compare by highest pair; `K-K-A-A-2-2` illegal.
7. **Plate** — two consecutive triples in natural order; compare by higher triple.

## Bombs (9 tiers, low → high)

Quadruple, quintuple, **straight flush**, sextuple, septuple, octuple, nonuple, decuple, **four-joker** (two black + two red, top).

- Same-count bombs: compare inside type using **level order**; level-rank bombs top their class when jokers cannot form that count.
- Straight flush: five same-suit consecutive in **natural** order; compare by high card; level-independent.
- Higher **bomb type** beats any lower type; within type, higher wins. Wilds can help build bombs except **four-joker**.

## Hand scoring and promotion

- Win types from finish order: **1-4** (+1 level), **1-3** (+2), **1-2** (+4) for the winning side (declarers next hand). Opponents who win also promote fully and become declarers (per classic Guan Dan promotion table in full rules).

## Tribute and return (from second hand)

- Losers pay **highest single non-wild** to winners; **return** a different card. Cancellation if red-joker conditions in full rules apply.
- **Opening lead** next hand: from tribute ranking / agreements / cancel rules as in full doc.

## Winning the game at level A

- Must win as **declarers** at A with **1-2** or **1-3**. **1-4** at A keeps declarers on A.
- Special **A-level** stay / demotion / three-failed-attempts / “all aces last play” cases: see full `doc/guandan_rules_en.md`.

## Card symbols (API / UI)

- Suits: `♠ ♥ ♦ ♣` + rank `A K Q J 10 9 … 2`; jokers `🃏R` / `🃏b`.
