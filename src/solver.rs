//! Counting solver.
//!
//! Flow across the board is strictly left-to-right and deterministic: a
//! column at a given scroll offset is just a partial function
//! `entry_row -> Option<exit_row>`. So "how many ways can this board be
//! solved" is a dynamic program over columns, not a search:
//!
//! ```text
//! dp[start_row] = 1
//! for each column:
//!     for each offset the column is allowed to take:
//!         for each row the water can arrive at:
//!             dp'[exit_row] += dp[entry_row]
//! solutions = dp[end_row]
//! ```
//!
//! `O(columns * rows^2)`, microseconds at any playable size. Crucially it
//! counts whole *scroll combinations*, not row-paths: two different offsets
//! of the same column that happen to route `a -> b` identically are two
//! different boards and are counted separately. That is exactly the
//! property the generator needs to guarantee "no other combination gives a
//! different solution".

use crate::game::{Piece, PipesGame, Side};

/// Where flow entering column `pieces` (in cyclic order, scrolled down by
/// `offset`) on the left at `entry_row` leaves on the right, or `None` if
/// it dead-ends inside the column.
pub(crate) fn column_transition(
    pieces: &[Piece],
    offset: usize,
    entry_row: usize,
) -> Option<usize> {
    let rows = pieces.len();
    let at = |row: usize| pieces[(row + rows - offset % rows) % rows];

    let mut row = entry_row;
    let mut entry = Side::Left;
    // Flow through a column is monotone (a vertical preserves direction and
    // a corner ends the run), so this cannot loop; the bound is defensive.
    for _ in 0..=rows {
        match at(row).exit(entry)? {
            Side::Right => return Some(row),
            Side::Up => {
                row = row.checked_sub(1)?;
                entry = Side::Down;
            }
            Side::Down => {
                row += 1;
                if row >= rows {
                    return None;
                }
                entry = Side::Up;
            }
            Side::Left => return None,
        }
    }
    debug_assert!(false, "column flow failed to terminate");
    None
}

impl PipesGame {
    /// The offsets column `col` may take: a locked column can only be where
    /// it already is, an unlocked one can be anywhere.
    fn allowed_offsets(&self, col: usize) -> Vec<usize> {
        if self.is_locked(col) {
            vec![self.offset(col)]
        } else {
            (0..self.rows()).collect()
        }
    }

    /// How many distinct combinations of column scrolls connect the inlet
    /// to the outlet, saturating at `cap` so a wide-open board can't
    /// overflow the count.
    ///
    /// `1` means the puzzle has exactly one solution.
    pub fn solution_count(&self, cap: usize) -> usize {
        let rows = self.rows();
        let mut dp = vec![0usize; rows];
        dp[self.start_row()] = 1;

        for col in 0..self.columns() {
            let mut next = vec![0usize; rows];
            for offset in self.allowed_offsets(col) {
                for (entry_row, &ways) in dp.iter().enumerate() {
                    if ways == 0 {
                        continue;
                    }
                    if let Some(exit_row) =
                        column_transition(self.base_column(col), offset, entry_row)
                    {
                        next[exit_row] = next[exit_row].saturating_add(ways).min(cap);
                    }
                }
            }
            dp = next;
        }

        dp[self.end_row()]
    }

    /// One winning set of column offsets, or `None` if the board cannot be
    /// solved from here. Locked columns keep their current offset.
    ///
    /// Backtracks the same table [`PipesGame::solution_count`] builds.
    pub fn solve(&self) -> Option<Vec<usize>> {
        let rows = self.rows();
        let columns = self.columns();

        // reachable[i][row]: the water can arrive at column i's left edge
        // at `row` using some choice of offsets for columns 0..i.
        let mut reachable = Vec::with_capacity(columns + 1);
        let mut current = vec![false; rows];
        current[self.start_row()] = true;
        reachable.push(current.clone());

        for col in 0..columns {
            let mut next = vec![false; rows];
            for offset in self.allowed_offsets(col) {
                for (entry_row, _) in current.iter().enumerate().filter(|(_, ok)| **ok) {
                    if let Some(exit_row) =
                        column_transition(self.base_column(col), offset, entry_row)
                    {
                        next[exit_row] = true;
                    }
                }
            }
            current = next;
            reachable.push(current.clone());
        }

        if !reachable[columns][self.end_row()] {
            return None;
        }

        // Walk back from the outlet, picking any offset that lands on a row
        // the prefix could actually reach.
        let mut offsets = vec![0usize; columns];
        let mut exit_row = self.end_row();
        for col in (0..columns).rev() {
            let mut found = None;
            'search: for offset in self.allowed_offsets(col) {
                for (entry_row, _) in reachable[col].iter().enumerate().filter(|(_, ok)| **ok) {
                    if column_transition(self.base_column(col), offset, entry_row) == Some(exit_row)
                    {
                        found = Some((offset, entry_row));
                        break 'search;
                    }
                }
            }
            let (offset, entry_row) = found?;
            offsets[col] = offset;
            exit_row = entry_row;
        }

        debug_assert_eq!(exit_row, self.start_row());
        Some(offsets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(
        rows: usize,
        cols: Vec<Vec<Piece>>,
        locked: Vec<bool>,
        start: usize,
        end: usize,
    ) -> PipesGame {
        let n = cols.len();
        PipesGame::from_parts(rows, cols, vec![0; n], locked, start, end)
    }

    #[test]
    fn transition_traces_a_corner_pair() {
        let column = vec![Piece::LeftDown, Piece::Vertical, Piece::UpRight];
        assert_eq!(column_transition(&column, 0, 0), Some(2));
        // Rows 1 and 2 have no left opening.
        assert_eq!(column_transition(&column, 0, 1), None);
        assert_eq!(column_transition(&column, 0, 2), None);
    }

    #[test]
    fn transition_reports_a_clipped_segment_as_a_dead_end() {
        // Scrolled by one, the three-cell segment straddles the edges: its
        // entry corner is at row 1 and its exit corner has wrapped to row 0,
        // so the run walks off the bottom.
        let column = vec![Piece::LeftDown, Piece::Vertical, Piece::UpRight];
        assert_eq!(column_transition(&column, 1, 1), None);
    }

    #[test]
    fn no_solution_is_counted_as_zero() {
        // A single locked column that drops the water one row, but the
        // outlet is level with the inlet.
        let g = game(
            2,
            vec![vec![Piece::LeftDown, Piece::UpRight]],
            vec![true],
            0,
            0,
        );
        assert_eq!(g.solution_count(4), 0);
        assert!(g.solve().is_none());
    }

    #[test]
    fn a_single_unlocked_column_with_one_matching_segment_is_unique() {
        // Three cells: a two-cell downward segment plus a lone horizontal.
        // Getting the water from row 0 to row 1 can only be done by the
        // downward segment sitting at rows 0-1, which is one offset.
        let g = game(
            3,
            vec![vec![Piece::LeftDown, Piece::UpRight, Piece::Horizontal]],
            vec![false],
            0,
            1,
        );
        assert_eq!(g.solution_count(4), 1);
        assert_eq!(g.solve(), Some(vec![0]));
    }

    #[test]
    fn duplicate_displacements_produce_a_second_solution() {
        // Two lone horizontals in the same column both map row 1 to row 1,
        // so two different offsets solve the board. This is exactly the
        // degenerate case the generator's filler constraint rules out.
        let g = game(
            3,
            vec![vec![Piece::Horizontal, Piece::Horizontal, Piece::Vertical]],
            vec![false],
            1,
            1,
        );
        assert_eq!(g.solution_count(4), 2);
    }

    #[test]
    fn locked_columns_do_not_multiply_the_count() {
        let g = game(
            3,
            vec![vec![Piece::Horizontal, Piece::Horizontal, Piece::Vertical]],
            vec![true],
            1,
            1,
        );
        assert_eq!(g.solution_count(4), 1);
    }

    #[test]
    fn solve_finds_offsets_across_several_columns() {
        let mut g = PipesGame::random(7, 6, 0.6, 99);
        let offsets = g.solve().expect("generated puzzles are solvable");
        for (col, &offset) in offsets.iter().enumerate() {
            if g.is_locked(col) {
                assert_eq!(g.offset(col), offset, "solve moved a locked column");
                continue;
            }
            // Bounded: one full wrap is always enough, and `rotate` stops
            // accepting input once the board is won.
            for _ in 0..g.rows() {
                if g.offset(col) == offset {
                    break;
                }
                g.rotate(col, crate::game::Rotation::Down);
            }
        }
        assert!(g.reaches_goal());
    }
}
