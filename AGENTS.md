# AGENTS.md

Instructions for AI coding agents working in this repository.

## What this is

`egui-pipes` is a self-contained Rust library implementing a pipe-connecting
puzzle for [egui](https://github.com/emilk/egui): renderer-agnostic game
logic plus a ready-to-use `egui::Widget`. It has no application of its own
beyond the demo in `examples/webapp.rs` — it's meant to be pulled into other
egui apps as a dependency.

The puzzle: water enters on the left edge and must leave at a marked row on
the right edge. **Pieces never rotate.** The only move is scrolling a whole
column down or up with wraparound; some columns are locked and can't be
scrolled at all.

"Pipe Dream" and "Pipe Mania" are trademarked product names for specific
commercial games in this genre — never use them in code, docs, or naming.
This project only refers to the genre generically ("pipe-connecting puzzle",
"pipes"). The same caution applies to "Net"/"Netwalk", which name a
*different* puzzle (rotating individual tiles) that this one is not.

## The model everything rests on

Six piece kinds, no tees and no crossings: `Horizontal`, `Vertical`, and the
four corners `LeftDown`, `UpRight`, `LeftUp`, `DownRight`.

A column is a **cyclic** partition of its `rows` cells into *segments*. A
segment takes flow in on the left at one end and out on the right at the
other: length 1 is a lone horizontal, length k>=2 is a corner, verticals, and
the matching corner. Two things fall out of this and both are load-bearing:

1. Because the partition is cyclic, scrolling can never produce a dangling
   corner. A segment that ends up straddling the top and bottom edges just
   reads as two dead ends against those edges, which is desirable, not a bug.
2. Flow is strictly left-to-right and deterministic, so a column at a given
   scroll offset is a partial function `entry_row -> Option<exit_row>`, and
   the whole board's solution count is a dynamic program rather than a search.

A segment's signed **displacement** (`exit_row - entry_row`) fixes its entire
shape. Given a required entry row, a segment of a given displacement fits at
exactly one offset. That single fact is what makes uniqueness cheap to both
enforce and check; see `generator.rs`'s module docs.

## Module layout

- `src/game.rs` — `Piece`, `Side`, `Rotation`, `GameStatus`, `FlowStep`,
  `PipesGame`, `write_segment`. Pure logic, no `egui::Widget`/`Ui` usage.
  Keep it that way: it should stay usable headlessly (for tests, or a non-egui
  renderer) without pulling in painting code. Water *animation state* lives
  here too (`advance`/`fill_progress`) so the widget can stay stateless.
- `src/solver.rs` — `column_transition`, plus `PipesGame::solution_count` and
  `PipesGame::solve`. The counting DP. It counts **scroll combinations**, not
  row-paths: two offsets of the same column that route `a -> b` identically
  are two different boards and count separately. Don't "optimize" that away;
  it is the property the uniqueness guarantee is stated in terms of.
- `src/generator.rs` — `pub(crate)` only. Lays the solution path first, fills
  the rest of each column with random segments under the displacement
  constraint, locks a share of the columns, scrambles the rest, then verifies
  uniqueness with the DP and retries. The module doc carries the full
  argument for why the filler constraint alone is *not* sufficient (it makes
  each column individually unambiguous but says nothing about a globally
  different route), which is why the DP still runs on every candidate.
- `src/widget.rs` — `PipesWidget`, `content_size`, `fit_cell_size`,
  `DEFAULT_WATER_COLOR`, and all painting/input handling. The only file
  allowed to depend on `egui::Ui`/`Painter`.
- `src/lib.rs` — thin re-export surface. `#![doc = include_str!("../README.md")]`
  means the crate-level docs are the README; keep the two in sync (usage
  snippets especially).
- `examples/webapp.rs` — a wasm demo app (via `xtask-wasm`), deployed to
  GitHub Pages by `.github/workflows/deploy.yml` on every push to `main`. Not
  part of the published crate (`Cargo.toml` excludes `/examples`).

## Rendering convention (easy to get wrong, so it's called out here)

A pipe is drawn as **three strokes along one centerline**, never as two
hand-placed parallel lines: a wide casing stroke, a narrower "bore" stroke in
the cell's own background color punched through it, then the water on top.
What the player sees is a hollow pipe with two parallel walls, and straights
and corners need no separate geometry.

The consequence to remember: **the bore stroke must match the color behind
the pipe**, which is per column — locked and unlocked columns have different
backgrounds. A constant bore color is the one mistake to check for after
touching `widget.rs`'s painting code; it shows up as a pale rectangle-free
halo inside locked columns.

## Building and testing

```sh
cargo check
cargo test --lib
cargo clippy -- -D warnings
cargo fmt --check
```

These four are exactly what `.github/workflows/ci.yml` runs on every push and
PR. Run them locally before committing.

The wasm demo isn't covered by `ci.yml` (only `deploy.yml` builds it, on push
to `main`). If you touch `examples/webapp.rs`, check it manually:

```sh
cargo check --target wasm32-unknown-unknown --example webapp
cargo clippy --target wasm32-unknown-unknown --example webapp -- -D warnings
cargo run --example webapp -- start     # local dev server, to actually play it
```

There is also an opt-in offline survey in `generator.rs` (every test
`#[ignore]`d) that measures the uniqueness hit-rate and how many scrolls the
shortest winning line costs, at a sample size the normal suite can't afford:

```sh
cargo test --lib --release -- --ignored --nocapture
```

## Conventions

- **No losing state and no timer.** Every scroll is reversible by scrolling
  back. If a scoring or challenge mode is ever added, it must be additive.
- A win is **latched**: once the water reaches the outlet the board stops
  accepting scrolls, so the banner and the completed run don't flicker away.
  Tests that scroll a board must not assume `rotate` keeps succeeding.
- `PipesGame::random` must always produce a board that has *at least* one
  solution by construction (the generator's own layout is one), and prefers
  one verified to have exactly one within `ATTEMPTS`. Both properties have
  tests; keep them.
- Generation is seeded and reproducible. Don't introduce unseeded randomness
  or time-dependence into `game.rs`/`generator.rs`.
- Add unit tests in `src/game.rs` for player-facing behavior (flow tracing,
  dead ends, scroll wrapping, locked columns, reset), in `src/solver.rs` for
  counting (hand-built boards with 0, 1 and 2 known solutions), and in
  `src/generator.rs` for generation properties (well-formed rings, solvable,
  uniquely solvable, not already solved). The painting code isn't unit
  testable the same way beyond its geometry helpers; verify it by eye via the
  wasm demo.
- Keep the public API renderer-agnostic where possible: prefer exposing
  queries (`piece_at`, `flow`, `is_locked`, `solve`) over raw field access, so
  the internal representation can change without breaking callers.

## Release process

Every published version gets a git tag and a changelog entry. To cut a
release:

1. Update `CHANGELOG.md`: move the `[Unreleased]` section's contents under a
   new `## [X.Y.Z] - YYYY-MM-DD` heading (Keep a Changelog format), and add
   the corresponding link reference at the bottom of the file.
2. Bump the `version` in `Cargo.toml` to match.
3. Run the full check suite above, plus `cargo package --list` as a final
   sanity check of what will actually be published.
4. `cargo publish`. This is irreversible per-version (a bad release can only
   be `cargo yank`-ed, not deleted) — don't skip step 3.
5. `git tag vX.Y.Z && git push && git push --tags`.

Follow SemVer: breaking changes (renamed/removed public items, changed method
signatures) require a major version bump (or a minor bump pre-1.0, per
SemVer's pre-1.0 rules).
