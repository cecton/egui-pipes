# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](keep_a_changelog) and this project adheres to [Semantic
Versioning](semver).

## [Unreleased]

## [0.1.4] - 2026-09-12

### Changed

- Filler segments now follow the same half-height cap as solution segments, in every column (locked ones included), so no incorrect path reads as one long run either
- In locked columns, filler segments never straddle the top/bottom edge: the scroll is frozen at offset 0, so every incorrect path in a locked column reads as a complete path with a beginning and an end, without requiring the rotation the player cannot perform

## [0.1.3] - 2026-09-12

### Changed

- `PipesGame::random` never locks the first or last column: the water enters and leaves the board there, so freezing either edge column reads as a dead board. The locked count itself is unchanged
- Solution segments are capped at half the column's height, rounded down, in every column (locked ones included), so a required path can no longer span most of a column as one long run. On boards of 3 rows or fewer, unlocked columns fall back to any exit where the cap would leave them no legal displacement; the uniqueness check still filters those boards

## [0.1.2] - 2026-09-10

### Added

- Drag-and-drop column scrolling: press an unlocked column and drag up or down; the column follows the pointer and commits one scroll per cell of travel, and any leftover fraction snaps home on release. Clicking and mouse-wheel scrolling behave exactly as before, and locked columns ignore drags. `PipesWidget::dragged` reports a drag-committed scroll to the embedding app, mirroring `PipesWidget::scrolled`

### Removed

- The web demo's drag-to-pan (and pinch/ctrl-wheel zoom) view on narrow viewports and touch devices, inherited from the egui-minesweeper template where big boards need it. Pipes boards always fit the viewport, so the board is now laid out directly and centered instead of living in an `egui::Scene`

## [0.1.1] - 2026-09-01

### Fixed

- A single mouse wheel notch no longer gets ignored. Wheel travel was measured against a fixed threshold in points, but a notch is worth a platform-dependent number of points (40 natively, 8 per line on web, times whatever the device reports), so one native notch fell short of the threshold and only a fast burst of notches scrolled a column. Wheel input is now read from the raw events, which carry the unit: one notch of a discrete wheel is one row, and only continuous devices (trackpads, browsers scrolling in pixel mode) are measured in points
- Partial trackpad travel is no longer thrown away on frames without a scroll event, so slow continuous scrolling adds up to a row instead of stalling

## [0.1.0] - 2026-09-01

### Added

- Initial release: `PipesGame` (game logic), `PipesWidget` (egui widget), `content_size`, `fit_cell_size` and `DEFAULT_WATER_COLOR`
- `PipesGame::random` generates a seeded puzzle that is always solvable by construction, and verifies with an exact counting solver that exactly one combination of column scrolls solves it
- `PipesGame::solution_count` and `PipesGame::solve`: a dynamic program over columns that counts whole scroll combinations rather than row-paths, so "unique solution" means one board state, not one route
- Column scrolling with wraparound: click for down, mouse wheel for either direction. Individual pieces never rotate
- Locked columns, controlled by `PipesGame::random`'s `locked_ratio`, drawn with a darker background, an outline and a padlock. At least two columns always stay scrollable however high the ratio is
- Live water: the flow is retraced after every scroll and animates from the inlet to the current dead end, so the board always shows how far the pipework actually reaches
- A wasm demo (`examples/webapp.rs`) with Beginner/Intermediate/Expert presets, deployed to GitHub Pages

[keep_a_changelog]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/cecton/egui-pipes/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/cecton/egui-pipes/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/cecton/egui-pipes/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/cecton/egui-pipes/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/cecton/egui-pipes/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/cecton/egui-pipes/releases/tag/v0.1.0
