# egui-pipes

[![crates.io](https://img.shields.io/crates/v/egui-pipes.svg)](https://crates.io/crates/egui-pipes)
[![docs.rs](https://docs.rs/egui-pipes/badge.svg)](https://docs.rs/egui-pipes)
[![deps.rs](https://deps.rs/repo/github/cecton/egui-pipes/status.svg)](https://deps.rs/repo/github/cecton/egui-pipes)
[![CI](https://github.com/cecton/egui-pipes/actions/workflows/ci.yml/badge.svg)](https://github.com/cecton/egui-pipes/actions/workflows/ci.yml)
[![Rust version](https://img.shields.io/badge/rustc-1.80+-ab6000.svg)](https://blog.rust-lang.org/2024/07/25/Rust-1.80.0.html)
[![License](https://img.shields.io/crates/l/egui-pipes.svg)](https://github.com/cecton/egui-pipes#license)
[![Changelog](https://img.shields.io/badge/changelog-Keep%20a%20Changelog%20v1.1.0-%23E05735)](CHANGELOG.md)
[![Live demo](https://img.shields.io/badge/demo-live-brightgreen)](https://cecton.github.io/egui-pipes)

A self-contained pipe-connecting puzzle game library for [egui](https://github.com/emilk/egui).

Water enters the board on the left and has to come out at a marked point on
the right. In between sit columns of pipe pieces: straights and 90-degree
corners. **The pieces themselves never rotate.** The only move is scrolling a
whole column down or up, wrapping around, until each column's outlet lines up
with the next column's inlet. Some columns are locked and cannot be scrolled
at all, so the movable ones have to be arranged around them.

The water is live feedback rather than a reward at the end: it always runs
from the inlet as far as the pipework currently reaches, and stops at the
first dead end. Connecting the last stretch is what carries it to the outlet.

## Features

- Pure game logic struct (`PipesGame`) with no `egui::Ui` dependency, usable headlessly or with any renderer
- Ready-to-use egui `Widget` (`PipesWidget`) drawing hollow, double-walled pipes that visibly fill with water
- Procedural, seeded generation (`PipesGame::random`) that always produces a solvable board and verifies, per board, that **exactly one** combination of column scrolls solves it
- An exact counting solver (`PipesGame::solution_count`, `PipesGame::solve`), fast enough to run on every generated candidate
- No dangling corners, by construction: every column is a cyclic partition into runs that take flow in on the left and out on the right, so scrolling can never produce an illegal board
- Click a column to scroll it down, or use the mouse wheel to scroll it either way
- No losing state and no timer: every scroll is undone by scrolling back
- Locked columns are drawn darker, outlined, and marked with a padlock

## Usage

Add the dependency:

```toml
[dependencies]
egui-pipes = "0.1"
```

Then use it in your egui app:

```rust,ignore
use egui_pipes::{PipesGame, PipesWidget};

// 7 columns, 6 rows, 60% of the columns locked, reproducible from a seed.
let mut game = PipesGame::random(7, 6, 0.6, 42);

// Inside your egui update/UI closure:
ui.add(PipesWidget::new(&mut game));

// Customize the water color and the win banner:
use egui::Color32;
ui.add(
    PipesWidget::new(&mut game)
        .water_color(Color32::from_rgb(0x3A, 0xC0, 0xA0))
        .win_message("Solved!"),
);
```

Check for a win after each frame:

```rust,ignore
use egui_pipes::GameStatus;

match game.status() {
    GameStatus::Playing => {}
    GameStatus::Won => println!("Connected!"),
}
```

To start over on the same puzzle:

```rust,ignore
game.reset();
```

To offer a hint, ask the solver where the columns need to end up:

```rust,ignore
if let Some(offsets) = game.solve() {
    // `offsets[col]` is where column `col` needs to end up; compare it with
    // `game.offset(col)` to see how far it still has to travel.
}
```

## How generation guarantees a single solution

Only a run's *entry* cell has an opening on its left, and a run of signed
displacement `delta` placed with its entry on row `a` always delivers the
water to row `a + delta`. So a column offers exactly as many valid scroll
positions for a required `a -> b` transition as it has runs of displacement
`b - a`.

The generator lays the intended solution down first, then fills the rest of
each column with random runs while keeping that displacement unique within
every scrollable column. That makes each column individually unambiguous; a
counting dynamic program over the whole board then confirms no *globally*
different route exists either, and the board is regenerated if one does.

## egui version compatibility

| egui-pipes | egui |
|------------|------|
| 0.1        | 0.35 |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
