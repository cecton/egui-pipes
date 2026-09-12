//! Puzzle generation. Internal: the only way in is [`crate::PipesGame::random`].
//!
//! # How a puzzle is built
//!
//! 1. Lay the *solution* down first. Pick a row for the inlet, then for each
//!    column pick the row the water should leave it by. That fixes one
//!    segment per column: its displacement is `exit - entry`, and its
//!    position is wherever those two rows are.
//! 2. Fill the rest of each column's cyclic ring with random segments, so
//!    every cell holds a piece and no corner dangles. Fillers follow the
//!    same half-height cap as the solution segment (see below), and in a
//!    locked column they never straddle the top/bottom edge: the scroll is
//!    frozen at offset 0, so every incorrect path in it must read as a
//!    complete path with a beginning and an end without any rotation.
//! 3. Store that solved layout as the column's stored order, i.e. the
//!    solution is "every column at offset 0".
//! 4. Lock a share of the columns, scroll the rest to random offsets.
//!    The first and last columns are never locked: the water enters and
//!    leaves the board there, so freezing either reads as a dead edge.
//! 5. Verify with [`crate::PipesGame::solution_count`] that exactly one
//!    combination of scrolls solves the board; retry if not.
//!
//! # The filler constraint
//!
//! Only a segment's *entry* cell has a left opening, so water can only get
//! into a column at a segment's entry, and a segment of displacement `delta`
//! placed with its entry on row `a` always spits the water out at `a + delta`.
//! Consequently a column offers exactly as many valid scroll offsets for a
//! required `a -> b` transition as it has segments of displacement `b - a`.
//!
//! So: in an **unlocked** column, no filler segment may share the solution
//! segment's displacement, or that column alone would accept two different
//! scrolls. This matters most in the case that would otherwise dominate,
//! `delta == 0`: a lone horizontal is the commonest filler there is, and any
//! second one in the same column would be just as good an answer. Rather
//! than ban length-1 fillers wholesale (which would make short columns
//! infeasible), unlocked columns simply never get a `delta == 0` solution
//! segment in the first place, so length-1 fillers stay available.
//!
//! Locked columns get no such constraint: their scroll never changes, so
//! duplicate displacements there cost nothing and keep the board varied.
//!
//! # The half-height constraint
//!
//! Every segment in every column (locked columns included, fillers
//! included) is capped at half the column's height, rounded down: a path
//! spanning most of a column reads as one long, uninteresting run. For
//! columns of 3 rows or fewer the cap leaves no displacement other than
//! `delta == 0`, which unlocked columns cannot take (see above), so
//! unlocked columns there fall back to any exit; the uniqueness check in
//! step 5 still rejects boards that would be ambiguous.
//!
//! The constraint only makes each column *individually* unambiguous. It does
//! not by itself rule out a globally different route (a wrong scroll in one
//! column landing the water on a row some later column happens to accept),
//! which is why step 5 still runs the real counting DP over the whole board.

use crate::game::{segment_delta, write_segment, Piece, PipesGame};

/// Candidate boards tried before giving up on uniqueness. The solved layout
/// is a solution by construction, so an exhausted budget still yields a
/// perfectly playable puzzle, just one that might have a second answer.
const ATTEMPTS: usize = 400;

struct Candidate {
    base: Vec<Vec<Piece>>,
    locked: Vec<bool>,
    start_row: usize,
    end_row: usize,
}

pub(crate) fn generate(columns: usize, rows: usize, locked_ratio: f32, seed: u64) -> PipesGame {
    assert!(columns >= 2, "a pipe puzzle needs at least 2 columns");
    assert!(rows >= 2, "a pipe puzzle needs at least 2 rows");

    let mut rng = fastrand::Rng::with_seed(seed);
    // Always leave at least two columns playable, however high the ratio:
    // one scrollable column is a puzzle you solve by mashing it.
    let locked_count = (columns as f32 * locked_ratio.clamp(0.0, 1.0)).round() as usize;
    let locked_count = locked_count.min(columns.saturating_sub(2));

    let mut fallback = None;
    for _ in 0..ATTEMPTS {
        let candidate = build_candidate(&mut rng, columns, rows, locked_count);

        let solved = PipesGame::from_parts(
            rows,
            candidate.base.clone(),
            vec![0; columns],
            candidate.locked.clone(),
            candidate.start_row,
            candidate.end_row,
        );
        debug_assert!(
            solved.reaches_goal(),
            "the all-zero layout is a solution by construction"
        );
        let unique = solved.solution_count(2) == 1;
        if !unique && fallback.is_some() {
            continue;
        }

        let offsets = scramble(&mut rng, rows, &candidate.locked);
        let game = PipesGame::from_parts(
            rows,
            candidate.base,
            offsets,
            candidate.locked,
            candidate.start_row,
            candidate.end_row,
        );
        if unique {
            return game;
        }
        fallback = Some(game);
    }

    fallback.expect("at least one candidate is built on the first attempt")
}

fn build_candidate(
    rng: &mut fastrand::Rng,
    columns: usize,
    rows: usize,
    locked_count: usize,
) -> Candidate {
    let mut locked = vec![false; columns];
    // The first and last columns are never lock candidates: the water
    // enters and leaves the board there, and a frozen edge column reads
    // as a dead board.
    let mut order: Vec<usize> = (1..columns - 1).collect();
    rng.shuffle(&mut order);
    for &col in order.iter().take(locked_count) {
        locked[col] = true;
    }

    let start_row = rng.usize(0..rows);
    let mut entry = start_row;
    let mut base = Vec::with_capacity(columns);
    for &is_locked in &locked {
        let exit = pick_exit(rng, rows, entry, is_locked);
        base.push(build_column(rng, rows, entry, exit, is_locked));
        entry = exit;
    }

    Candidate {
        base,
        locked,
        start_row,
        end_row: entry,
    }
}

/// The row the solution leaves a column by. Two constraints:
///
/// - The segment spans at most half the column's height, rounded down
///   (`len = |exit - entry| + 1 <= rows / 2`), so no required path reads
///   as one long run down the column.
/// - Unlocked columns never go straight across: a `delta == 0` solution
///   segment would forbid every length-1 filler in that column (see the
///   module docs).
///
/// With 3 rows or fewer the cap leaves unlocked columns no displacement
/// but `delta == 0`, so they fall back to any exit there; the whole-board
/// uniqueness check still rejects ambiguous boards.
fn pick_exit(rng: &mut fastrand::Rng, rows: usize, entry: usize, is_locked: bool) -> usize {
    let max_len = (rows / 2).max(1);
    let max_delta = max_len - 1;

    let candidates: Vec<usize> = (0..rows)
        .filter(|&exit| {
            let delta = exit.abs_diff(entry);
            delta <= max_delta && (is_locked || delta != 0 || max_delta == 0)
        })
        .collect();

    match candidates.as_slice() {
        [] => rng.usize(0..rows),
        [only] => *only,
        _ => candidates[rng.usize(0..candidates.len())],
    }
}

/// One column, at its solved scroll: the solution segment between `entry`
/// and `exit`, and random segments filling the rest of the ring.
///
/// Fillers never exceed half the column's height, just like the solution
/// segment. In a locked column they additionally never straddle the
/// top/bottom edge: the column's scroll is frozen at offset 0, so a
/// wrapped filler would sit there forever as two dangling corners pointing
/// off the board instead of reading as a path with a beginning and an end.
/// Unlocked columns keep the cyclic tiling; there the wrap is just
/// something the player rotates into view.
fn build_column(
    rng: &mut fastrand::Rng,
    rows: usize,
    entry: usize,
    exit: usize,
    is_locked: bool,
) -> Vec<Piece> {
    let top = entry.min(exit);
    let len = entry.abs_diff(exit) + 1;
    let down = exit >= entry;
    let max_len = (rows / 2).max(1);

    let mut out = vec![Piece::Horizontal; rows];
    write_segment(&mut out, top, len, down);

    // The displacement ban only applies when the solution segment is
    // itself displacing: a `delta == 0` solution (possible on unlocked
    // columns via the tiny-board fallback) leaves every filler shape
    // equally guilty, and the uniqueness check decides whether the board
    // stands.
    let forbidden = (!is_locked && segment_delta(len, down) != 0).then(|| segment_delta(len, down));

    if is_locked {
        // The solution segment splits the column into two visible runs,
        // below and above itself; tile each one without crossing its
        // edges.
        for (run_top, run_len) in [((top + len) % rows, rows - top - len), (0, top)] {
            let mut cursor = run_top;
            let mut remaining = run_len;
            while remaining > 0 {
                let (filler_len, filler_down) = pick_filler(rng, remaining, max_len, forbidden);
                write_segment(&mut out, cursor, filler_len, filler_down);
                cursor += filler_len;
                remaining -= filler_len;
            }
        }
    } else {
        // Whatever the solution segment doesn't use is one contiguous arc
        // of the ring, starting just past its bottom end.
        let mut cursor = (top + len) % rows;
        let mut remaining = rows - len;
        while remaining > 0 {
            let (filler_len, filler_down) = pick_filler(rng, remaining, max_len, forbidden);
            write_segment(&mut out, cursor, filler_len, filler_down);
            cursor = (cursor + filler_len) % rows;
            remaining -= filler_len;
        }
    }

    out
}

/// A filler segment's shape, weighted toward short runs so a column reads as
/// a varied stack rather than one long vertical, and never taking the
/// displacement the solution segment already owns. `max_len` caps the
/// segment's length at half the column's height, like the solution segment.
fn pick_filler(
    rng: &mut fastrand::Rng,
    remaining: usize,
    max_len: usize,
    forbidden: Option<isize>,
) -> (usize, bool) {
    let mut options: Vec<(usize, bool)> = Vec::new();
    for len in 1..=remaining.min(max_len) {
        for down in [true, false] {
            // A one-cell segment is a lone horizontal; it has no two variants.
            if len == 1 && !down {
                continue;
            }
            if forbidden == Some(segment_delta(len, down)) {
                continue;
            }
            let weight = match len {
                1 => 6,
                2 => 4,
                3 => 2,
                _ => 1,
            };
            for _ in 0..weight {
                options.push((len, down));
            }
        }
    }

    match options.len() {
        // Unreachable while `pick_exit` keeps unlocked columns off
        // `delta == 0`, since a length-1 filler is then always legal. If it
        // ever were, filling the arc in one piece keeps the column
        // well-formed and the uniqueness check simply rejects the board.
        0 => {
            debug_assert!(false, "no legal filler shape for {remaining} cells");
            (remaining, true)
        }
        n => options[rng.usize(0..n)],
    }
}

/// Random starting scrolls for the unlocked columns. Since the solution is
/// "every column at offset 0", a deal that leaves every unlocked column at 0
/// would hand the player a finished puzzle, so it is re-rolled.
fn scramble(rng: &mut fastrand::Rng, rows: usize, locked: &[bool]) -> Vec<usize> {
    loop {
        let offsets: Vec<usize> = locked
            .iter()
            .map(|&is_locked| if is_locked { 0 } else { rng.usize(0..rows) })
            .collect();
        if locked
            .iter()
            .zip(&offsets)
            .any(|(&is_locked, &offset)| !is_locked && offset != 0)
        {
            return offsets;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameStatus, Rotation, Side};

    /// Every cell holds a piece, and every opening has a partner: a corner
    /// or vertical pointing up must meet one pointing down in the cell
    /// above, cyclically. This is the "no dangling angle" rule.
    fn assert_well_formed_ring(column: &[Piece]) {
        let rows = column.len();
        for row in 0..rows {
            let above = column[(row + rows - 1) % rows];
            assert_eq!(
                column[row].has(Side::Up),
                above.has(Side::Down),
                "row {row} does not match the cell above it in {column:?}"
            );
        }
    }

    fn presets() -> [(usize, usize, f32); 3] {
        [(6, 5, 0.4), (7, 6, 0.4), (9, 7, 0.4)]
    }

    #[test]
    fn every_column_is_a_well_formed_ring() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..40 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                for col in 0..game.columns() {
                    assert_well_formed_ring(game.base_column(col));
                }
            }
        }
    }

    #[test]
    fn the_first_and_last_columns_are_never_locked() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..40 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                assert!(!game.is_locked(0), "{columns}x{rows} seed {seed}");
                assert!(!game.is_locked(columns - 1), "{columns}x{rows} seed {seed}");
            }
        }
    }

    /// Walks the solved layout column by column, following the water from
    /// the inlet, and returns each column's `(entry_row, exit_row)`.
    fn solution_path(game: &PipesGame) -> Vec<(usize, usize)> {
        let rows = game.rows();
        let mut path = Vec::with_capacity(game.columns());
        let mut row = game.start_row();
        for col in 0..game.columns() {
            let column = game.base_column(col);
            let entry = row;
            let mut side = Side::Left;
            let mut cur = entry;
            loop {
                let next = column[cur].exit(side).expect("solution path is connected");
                match next {
                    Side::Right => break,
                    Side::Down => {
                        cur = (cur + 1) % rows;
                        side = Side::Up;
                    }
                    Side::Up => {
                        cur = (cur + rows - 1) % rows;
                        side = Side::Down;
                    }
                    Side::Left => unreachable!("flow never re-enters leftward"),
                }
            }
            path.push((entry, cur));
            row = cur;
        }
        assert_eq!(row, game.end_row(), "path must end at the outlet");
        path
    }

    #[test]
    fn solution_segments_never_exceed_half_the_column_height() {
        // The presets plus a taller board; rows of 4+ always give the cap
        // room to bite (below that the tiny-board fallback may exceed it).
        for (columns, rows, ratio) in [(6, 5, 0.4), (7, 6, 0.4), (9, 7, 0.4), (5, 9, 0.3)] {
            for seed in 0..60 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                let max_len = rows / 2;
                for (col, &(entry, exit)) in solution_path(&game).iter().enumerate() {
                    let len = entry.abs_diff(exit) + 1;
                    assert!(
                        len <= max_len,
                        "{columns}x{rows} seed {seed}: column {col} \
                         solution spans {entry} -> {exit} ({len} > {max_len})"
                    );
                }
            }
        }
    }

    /// Recovers every segment in a column at its solved scroll (offset 0):
    /// scanning top to bottom, each cell with a left opening starts one
    /// segment, whose flow is followed (up or down) to its right opening.
    /// Returns `(len, wraps)` pairs; `wraps` is whether the segment
    /// straddles the top/bottom edge (occupies both rows 0 and rows - 1).
    fn all_segments(column: &[Piece]) -> Vec<(usize, bool)> {
        let rows = column.len();
        let mut segments = Vec::new();
        let mut consumed = vec![false; rows];
        for start in 0..rows {
            if consumed[start] || !column[start].has(Side::Left) {
                continue;
            }
            let mut len = 0;
            let mut saw_top = false;
            let mut saw_bottom = false;
            let mut closed = false;
            let mut cur = start;
            let mut entry = Side::Left;
            for _ in 0..rows {
                consumed[cur] = true;
                len += 1;
                saw_top |= cur == 0;
                saw_bottom |= cur == rows - 1;
                let exit = column[cur]
                    .exit(entry)
                    .expect("a left opening always flows somewhere");
                match exit {
                    Side::Right => {
                        closed = true;
                        break;
                    }
                    Side::Down => {
                        cur = (cur + 1) % rows;
                        entry = Side::Up;
                    }
                    Side::Up => {
                        cur = (cur + rows - 1) % rows;
                        entry = Side::Down;
                    }
                    Side::Left => unreachable!("flow never re-enters leftward"),
                }
            }
            assert!(closed, "every segment closes within one lap of the ring");
            let wraps = saw_top && saw_bottom;
            segments.push((len, wraps));
        }
        segments
    }

    #[test]
    fn every_segment_stays_within_half_the_column_height() {
        for (columns, rows, ratio) in [(6, 5, 0.4), (7, 6, 0.4), (9, 7, 0.4), (5, 9, 0.3)] {
            for seed in 0..60 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                let max_len = rows / 2;
                for col in 0..game.columns() {
                    for &(len, _) in &all_segments(game.base_column(col)) {
                        assert!(
                            len <= max_len,
                            "{columns}x{rows} seed {seed}: column {col} \
                             has a segment of {len} > {max_len}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn locked_columns_never_wrap_a_segment_over_the_edge() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..60 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                for col in 0..game.columns() {
                    if !game.is_locked(col) {
                        continue;
                    }
                    for &(len, wraps) in &all_segments(game.base_column(col)) {
                        assert!(
                            !wraps,
                            "{columns}x{rows} seed {seed}: locked column {col} \
                             wraps a {len}-cell segment over the edge"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn generated_boards_are_solvable() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..40 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                assert!(
                    game.solve().is_some(),
                    "{columns}x{rows} seed {seed} has no solution"
                );
            }
        }
    }

    #[test]
    fn generated_boards_have_exactly_one_solution() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..60 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                assert_eq!(
                    game.solution_count(4),
                    1,
                    "{columns}x{rows} seed {seed} is not uniquely solvable"
                );
            }
        }
    }

    #[test]
    fn generated_boards_do_not_start_solved() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..40 {
                let game = PipesGame::random(columns, rows, ratio, seed);
                assert_eq!(game.status(), GameStatus::Playing);
                assert!(!game.reaches_goal());
            }
        }
    }

    #[test]
    fn the_locked_ratio_is_honoured_and_leaves_room_to_play() {
        for (columns, rows, ratio) in presets() {
            let game = PipesGame::random(columns, rows, ratio, 7);
            let locked = (0..columns).filter(|&c| game.is_locked(c)).count();
            assert_eq!(locked, (columns as f32 * ratio).round() as usize);
            assert!(columns - locked >= 2);
        }
    }

    #[test]
    fn an_extreme_locked_ratio_still_leaves_two_columns() {
        let game = PipesGame::random(6, 5, 1.0, 3);
        let unlocked = (0..game.columns()).filter(|&c| !game.is_locked(c)).count();
        assert_eq!(unlocked, 2);
    }

    #[test]
    fn playing_the_solution_wins() {
        for (columns, rows, ratio) in presets() {
            for seed in 0..20 {
                let mut game = PipesGame::random(columns, rows, ratio, seed);
                let offsets = game.solve().expect("solvable");
                for (col, &offset) in offsets.iter().enumerate() {
                    for _ in 0..rows {
                        if game.offset(col) == offset {
                            break;
                        }
                        game.rotate(col, Rotation::Down);
                    }
                }
                assert_eq!(game.status(), GameStatus::Won);
            }
        }
    }

    #[test]
    fn smallest_playable_board_works() {
        let game = PipesGame::random(2, 2, 0.6, 1);
        assert_eq!(game.columns(), 2);
        assert!(game.solve().is_some());
    }

    /// Opt-in offline harness for measuring how often the uniqueness gate
    /// actually fires, at a sample size the test suite can't afford. Not run
    /// by `cargo test` or CI.
    ///
    /// ```sh
    /// cargo test --lib -- --ignored --nocapture uniqueness_rate
    /// ```
    mod difficulty_survey {
        use super::*;

        #[test]
        #[ignore = "offline survey, not a correctness check"]
        fn uniqueness_rate() {
            for (columns, rows, ratio) in presets() {
                let mut unique = 0;
                let samples = 2000;
                for seed in 0..samples {
                    let game = PipesGame::random(columns, rows, ratio, seed);
                    if game.solution_count(4) == 1 {
                        unique += 1;
                    }
                }
                println!(
                    "{columns}x{rows} locked {ratio}: {unique}/{samples} uniquely solvable \
                     ({:.1}%)",
                    100.0 * unique as f32 / samples as f32
                );
            }
        }

        /// How many scrolls the shortest winning line actually costs, as a
        /// crude proxy for how much work a board asks of the player.
        #[test]
        #[ignore = "offline survey, not a correctness check"]
        fn scroll_cost() {
            for (columns, rows, ratio) in presets() {
                let mut total = 0usize;
                let mut worst = 0usize;
                let samples = 500;
                for seed in 0..samples {
                    let game = PipesGame::random(columns, rows, ratio, seed);
                    let offsets = game.solve().expect("solvable");
                    let cost: usize = (0..game.columns())
                        .map(|col| {
                            let delta = (offsets[col] + rows - game.offset(col)) % rows;
                            delta.min(rows - delta)
                        })
                        .sum();
                    total += cost;
                    worst = worst.max(cost);
                }
                println!(
                    "{columns}x{rows} locked {ratio}: {:.1} scrolls average, {worst} worst",
                    total as f32 / samples as f32
                );
            }
        }
    }
}
