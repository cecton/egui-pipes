# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](keep_a_changelog) and this project adheres to [Semantic
Versioning](semver).

## [Unreleased]

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
[Unreleased]: https://github.com/cecton/egui-pipes/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cecton/egui-pipes/releases/tag/v0.1.0
