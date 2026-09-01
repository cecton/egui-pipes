//! Pure game logic: pieces, columns, flow tracing and the [`PipesGame`]
//! state machine. Nothing here touches `egui::Ui`/`Painter` (that lives in
//! `src/widget.rs`), so the whole model stays usable headlessly.

/// One of a cell's four edges.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

impl Side {
    /// The edge directly across the cell.
    pub fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

/// A single pipe piece. Only straights and 90-degree corners exist: there
/// are no tees and no crossings, so flow through a piece is always a
/// function of the edge it entered by.
///
/// Pieces are never rotated by the player. The only move in the game is
/// scrolling a whole column (see [`PipesGame::rotate`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Piece {
    /// Straight, joining the left and right edges.
    Horizontal,
    /// Straight, joining the top and bottom edges.
    Vertical,
    /// Corner joining the left and bottom edges.
    LeftDown,
    /// Corner joining the top and right edges.
    UpRight,
    /// Corner joining the left and top edges.
    LeftUp,
    /// Corner joining the bottom and right edges.
    DownRight,
}

impl Piece {
    /// The two edges this piece opens onto.
    pub fn sides(self) -> [Side; 2] {
        match self {
            Self::Horizontal => [Side::Left, Side::Right],
            Self::Vertical => [Side::Up, Side::Down],
            Self::LeftDown => [Side::Left, Side::Down],
            Self::UpRight => [Side::Up, Side::Right],
            Self::LeftUp => [Side::Left, Side::Up],
            Self::DownRight => [Side::Down, Side::Right],
        }
    }

    /// Whether this piece opens onto `side`.
    pub fn has(self, side: Side) -> bool {
        let [a, b] = self.sides();
        a == side || b == side
    }

    /// Whether this piece turns the flow 90 degrees.
    pub fn is_corner(self) -> bool {
        !matches!(self, Self::Horizontal | Self::Vertical)
    }

    /// Which edge flow entering through `entry` leaves by, or `None` when
    /// the piece has no opening on `entry` at all (so the flow dead-ends
    /// against this piece's wall rather than passing through it).
    pub fn exit(self, entry: Side) -> Option<Side> {
        let [a, b] = self.sides();
        if a == entry {
            Some(b)
        } else if b == entry {
            Some(a)
        } else {
            None
        }
    }
}

/// Which way a column scroll moves its pieces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rotation {
    /// Every piece moves down one row; the bottom one wraps to the top.
    Down,
    /// Every piece moves up one row; the top one wraps to the bottom.
    Up,
}

/// Whether the puzzle is still being played or has been solved. There is no
/// losing state: a scroll is always undoable by scrolling back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GameStatus {
    #[default]
    Playing,
    Won,
}

/// One cell the water passes through, in flow order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowStep {
    pub col: usize,
    pub row: usize,
    /// The edge the water enters this cell by.
    pub entry: Side,
    /// The edge the water leaves this cell by. On the final step this leads
    /// nowhere: into a wall, off the board's top/bottom edge, or out of the
    /// right edge at the wrong row.
    pub exit: Side,
}

/// Writes one well-formed segment into a column's cyclic piece buffer.
///
/// A segment is `len` cells long starting at cyclic position `top`, and
/// takes flow in on its left at one end and out on its right at the other:
/// `down` puts the entry at the top, otherwise it is at the bottom.
///
/// The useful way to think about a segment is its signed **displacement**
/// `delta = exit_row - entry_row`, which is `len - 1` when `down` and
/// `-(len - 1)` otherwise (so a lone horizontal is `delta == 0`). One
/// number fixes the entire shape, which is what makes uniqueness cheap to
/// reason about: given a required entry row, a segment of a given
/// displacement fits at exactly one scroll offset. See `generator.rs`.
///
/// This is also the invariant the whole puzzle rests on: a corner never
/// dangles, because it is only ever emitted as one end of a run whose other
/// end carries the matching corner.
pub(crate) fn write_segment(out: &mut [Piece], top: usize, len: usize, down: bool) {
    let rows = out.len();
    debug_assert!(len >= 1 && len <= rows);

    if len == 1 {
        out[top] = Piece::Horizontal;
        return;
    }

    let bottom = (top + len - 1) % rows;
    if down {
        out[top] = Piece::LeftDown;
        out[bottom] = Piece::UpRight;
    } else {
        out[bottom] = Piece::LeftUp;
        out[top] = Piece::DownRight;
    }
    for j in 1..len - 1 {
        out[(top + j) % rows] = Piece::Vertical;
    }
}

/// The signed displacement of a segment of the given shape. See
/// [`write_segment`].
pub(crate) fn segment_delta(len: usize, down: bool) -> isize {
    let span = len as isize - 1;
    if down {
        span
    } else {
        -span
    }
}

/// A pipe puzzle: a `columns` x `rows` board of pipe pieces, plus a water
/// inlet on the left edge and an outlet on the right edge.
///
/// Every column holds a cyclic partition of its `rows` cells into segments,
/// each of which takes flow in on the left and out on the right. Because
/// the partition is cyclic, scrolling a column can never produce a dangling
/// corner: a segment that ends up straddling the top and bottom edges just
/// reads as two dead ends against those edges.
///
/// Build one with [`PipesGame::random`].
pub struct PipesGame {
    columns: usize,
    rows: usize,
    /// Per column, its pieces in cyclic order. `base[c][i]` is displayed at
    /// row `(i + offsets[c]) % rows`.
    base: Vec<Vec<Piece>>,
    offsets: Vec<usize>,
    initial_offsets: Vec<usize>,
    locked: Vec<bool>,
    start_row: usize,
    end_row: usize,
    status: GameStatus,
    moves: usize,
    flow: Vec<FlowStep>,
    reaches_goal: bool,
    fill: f32,
}

impl PipesGame {
    /// Cells per second the water advances along the connected run.
    const FILL_SPEED: f32 = 7.0;

    pub(crate) fn from_parts(
        rows: usize,
        base: Vec<Vec<Piece>>,
        offsets: Vec<usize>,
        locked: Vec<bool>,
        start_row: usize,
        end_row: usize,
    ) -> Self {
        let columns = base.len();
        debug_assert_eq!(offsets.len(), columns);
        debug_assert_eq!(locked.len(), columns);
        debug_assert!(base.iter().all(|col| col.len() == rows));
        let mut game = Self {
            columns,
            rows,
            base,
            initial_offsets: offsets.clone(),
            offsets,
            locked,
            start_row,
            end_row,
            status: GameStatus::Playing,
            moves: 0,
            flow: Vec::new(),
            reaches_goal: false,
            fill: 0.0,
        };
        game.recompute();
        game
    }

    /// Generates a random puzzle.
    ///
    /// `locked_ratio` is the fraction of columns the player cannot scroll
    /// (clamped so at least two columns stay playable). The result always
    /// has at least one solution by construction, and the generator prefers
    /// one it has verified to have *exactly* one; see `generator.rs`.
    ///
    /// # Panics
    ///
    /// Panics if `columns` is less than 2 or `rows` is less than 2.
    pub fn random(columns: usize, rows: usize, locked_ratio: f32, seed: u64) -> Self {
        crate::generator::generate(columns, rows, locked_ratio, seed)
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The row the water enters column 0 at, from the left edge.
    pub fn start_row(&self) -> usize {
        self.start_row
    }

    /// The row the water must leave the last column at to win.
    pub fn end_row(&self) -> usize {
        self.end_row
    }

    /// Whether `col` is frozen: locked columns cannot be scrolled and are
    /// drawn differently.
    pub fn is_locked(&self, col: usize) -> bool {
        self.locked[col]
    }

    /// The piece currently displayed at `(col, row)`.
    pub fn piece_at(&self, col: usize, row: usize) -> Piece {
        self.base[col][self.base_index(col, row)]
    }

    /// How far column `col` has been scrolled down from its stored order.
    pub(crate) fn offset(&self, col: usize) -> usize {
        self.offsets[col]
    }

    pub(crate) fn base_column(&self, col: usize) -> &[Piece] {
        &self.base[col]
    }

    fn base_index(&self, col: usize, row: usize) -> usize {
        (row + self.rows - self.offsets[col] % self.rows) % self.rows
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    /// Number of column scrolls made since the puzzle started.
    pub fn moves(&self) -> usize {
        self.moves
    }

    /// The cells the water currently occupies, in flow order, starting at
    /// the inlet. Always non-empty unless the very first piece walls it off.
    pub fn flow(&self) -> &[FlowStep] {
        &self.flow
    }

    /// Whether the connected run currently reaches the outlet. This is the
    /// win condition; the water still needs a moment to actually travel
    /// there (see [`PipesGame::fill_progress`]).
    pub fn reaches_goal(&self) -> bool {
        self.reaches_goal
    }

    /// How much of [`PipesGame::flow`] the water has actually covered, in
    /// cells. Animation state lives here rather than in the widget so the
    /// crate stays renderer-agnostic.
    pub fn fill_progress(&self) -> f32 {
        self.fill
    }

    /// Whether the water is still travelling and the view needs repainting.
    pub fn is_animating(&self) -> bool {
        self.fill < self.flow.len() as f32
    }

    /// Advances the water by `dt` seconds. A scroll that shortens the run
    /// snaps the water back immediately rather than draining it slowly, so
    /// the board always reads as the truth about the current layout.
    pub fn advance(&mut self, dt: f32) {
        let target = self.flow.len() as f32;
        if self.fill > target {
            self.fill = target;
        } else if self.fill < target {
            self.fill = (self.fill + dt * Self::FILL_SPEED).min(target);
        }
    }

    /// Scrolls a column by one row, wrapping. Returns whether anything
    /// moved: locked columns, out-of-range indices and an already-solved
    /// puzzle are all no-ops.
    pub fn rotate(&mut self, col: usize, direction: Rotation) -> bool {
        if col >= self.columns || self.locked[col] || self.status == GameStatus::Won {
            return false;
        }
        self.offsets[col] = match direction {
            Rotation::Down => (self.offsets[col] + 1) % self.rows,
            Rotation::Up => (self.offsets[col] + self.rows - 1) % self.rows,
        };
        self.moves += 1;
        self.recompute();
        true
    }

    /// Puts every column back where it started, keeping the same puzzle.
    pub fn reset(&mut self) {
        self.offsets.clone_from(&self.initial_offsets);
        self.moves = 0;
        self.status = GameStatus::Playing;
        self.fill = 0.0;
        self.recompute();
    }

    /// Retraces the water and re-checks the win condition. Called after
    /// every change to `offsets`.
    fn recompute(&mut self) {
        self.flow.clear();
        let mut col = 0;
        let mut row = self.start_row;
        let mut entry = Side::Left;

        while col < self.columns {
            let Some(exit) = self.piece_at(col, row).exit(entry) else {
                break;
            };
            self.flow.push(FlowStep {
                col,
                row,
                entry,
                exit,
            });
            match exit {
                Side::Right => {
                    col += 1;
                    entry = Side::Left;
                }
                Side::Up => {
                    let Some(next) = row.checked_sub(1) else {
                        break;
                    };
                    row = next;
                    entry = Side::Down;
                }
                Side::Down => {
                    row += 1;
                    if row >= self.rows {
                        break;
                    }
                    entry = Side::Up;
                }
                // Unreachable for well-formed pieces: nothing sends flow
                // back out the way it came in.
                Side::Left => break,
            }
        }

        self.reaches_goal = col == self.columns && row == self.end_row;
        // Latch the win: once solved the board stops accepting scrolls, so
        // the banner and the finished water run stay put.
        if self.reaches_goal {
            self.status = GameStatus::Won;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a board straight from per-column piece lists (row order, no
    /// scroll applied), with every column unlocked.
    fn board(rows: usize, cols: Vec<Vec<Piece>>, start: usize, end: usize) -> PipesGame {
        let n = cols.len();
        PipesGame::from_parts(rows, cols, vec![0; n], vec![false; n], start, end)
    }

    #[test]
    fn piece_exit_is_symmetric() {
        for piece in [
            Piece::Horizontal,
            Piece::Vertical,
            Piece::LeftDown,
            Piece::UpRight,
            Piece::LeftUp,
            Piece::DownRight,
        ] {
            let [a, b] = piece.sides();
            assert_eq!(piece.exit(a), Some(b));
            assert_eq!(piece.exit(b), Some(a));
            for side in [Side::Left, Side::Right, Side::Up, Side::Down] {
                if side != a && side != b {
                    assert_eq!(piece.exit(side), None);
                }
            }
        }
    }

    #[test]
    fn write_segment_shapes() {
        let mut out = vec![Piece::Horizontal; 4];
        write_segment(&mut out, 0, 4, true);
        assert_eq!(
            out,
            vec![
                Piece::LeftDown,
                Piece::Vertical,
                Piece::Vertical,
                Piece::UpRight
            ]
        );

        let mut out = vec![Piece::Horizontal; 3];
        write_segment(&mut out, 0, 3, false);
        assert_eq!(out, vec![Piece::DownRight, Piece::Vertical, Piece::LeftUp]);

        let mut out = vec![Piece::Vertical; 1];
        write_segment(&mut out, 0, 1, true);
        assert_eq!(out, vec![Piece::Horizontal]);
    }

    #[test]
    fn write_segment_wraps_around_the_buffer() {
        // A three-cell downward segment whose top sits on the last row: it
        // wraps, which is exactly what a scrolled column looks like.
        let mut out = vec![Piece::Horizontal; 4];
        write_segment(&mut out, 3, 3, true);
        assert_eq!(out[3], Piece::LeftDown);
        assert_eq!(out[0], Piece::Vertical);
        assert_eq!(out[1], Piece::UpRight);
    }

    #[test]
    fn segment_delta_matches_the_written_shape() {
        assert_eq!(segment_delta(1, true), 0);
        assert_eq!(segment_delta(3, true), 2);
        assert_eq!(segment_delta(3, false), -2);
    }

    #[test]
    fn straight_run_wins() {
        let game = board(
            1,
            vec![vec![Piece::Horizontal], vec![Piece::Horizontal]],
            0,
            0,
        );
        assert!(game.reaches_goal());
        assert_eq!(game.status(), GameStatus::Won);
        assert_eq!(game.flow().len(), 2);
    }

    #[test]
    fn corner_run_changes_row() {
        // Column 0 takes the water in at row 0 and drops it to row 1;
        // column 1 carries it straight out at row 1.
        let game = board(
            2,
            vec![
                vec![Piece::LeftDown, Piece::UpRight],
                vec![Piece::Vertical, Piece::Horizontal],
            ],
            0,
            1,
        );
        assert!(game.reaches_goal());
        let rows: Vec<_> = game.flow().iter().map(|s| (s.col, s.row)).collect();
        assert_eq!(rows, vec![(0, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn wrong_exit_row_is_not_a_win() {
        let game = board(
            2,
            vec![
                vec![Piece::LeftDown, Piece::UpRight],
                vec![Piece::Vertical, Piece::Horizontal],
            ],
            0,
            0, // the water actually leaves at row 1
        );
        assert!(!game.reaches_goal());
        assert_eq!(game.status(), GameStatus::Playing);
    }

    #[test]
    fn dead_end_at_a_column_boundary() {
        // Column 1's row 0 is a vertical, so it has no left opening for the
        // water arriving from column 0.
        let game = board(
            2,
            vec![
                vec![Piece::Horizontal, Piece::Horizontal],
                vec![Piece::Vertical, Piece::Vertical],
            ],
            0,
            0,
        );
        assert!(!game.reaches_goal());
        assert_eq!(game.flow().len(), 1);
    }

    #[test]
    fn dead_end_off_the_top_edge() {
        // A segment clipped by the top edge: the water turns up, runs
        // through the vertical, and then walks off the board.
        let game = board(
            2,
            vec![
                vec![Piece::Vertical, Piece::LeftUp],
                vec![Piece::Horizontal, Piece::Horizontal],
            ],
            1,
            1,
        );
        assert!(!game.reaches_goal());
        let visited: Vec<_> = game.flow().iter().map(|s| (s.col, s.row)).collect();
        assert_eq!(visited, vec![(0, 1), (0, 0)]);
    }

    #[test]
    fn scrolling_wraps_both_ways() {
        // The outlet is on row 1, so the horizontal on row 0 never solves
        // the board and the win latch stays open for the whole test.
        let mut game = board(
            3,
            vec![vec![Piece::Horizontal, Piece::Vertical, Piece::Vertical]],
            0,
            1,
        );
        assert_eq!(game.piece_at(0, 0), Piece::Horizontal);

        assert!(game.rotate(0, Rotation::Down));
        assert_eq!(game.piece_at(0, 1), Piece::Horizontal);
        assert_eq!(game.piece_at(0, 0), Piece::Vertical);

        assert!(game.rotate(0, Rotation::Up));
        assert_eq!(game.piece_at(0, 0), Piece::Horizontal);

        // Wrapping past the top brings the bottom piece around.
        assert!(game.rotate(0, Rotation::Up));
        assert_eq!(game.piece_at(0, 2), Piece::Horizontal);
        assert_eq!(game.moves(), 3);
    }

    #[test]
    fn locked_columns_reject_rotation() {
        let mut game = PipesGame::from_parts(
            2,
            vec![vec![Piece::Vertical, Piece::Horizontal]],
            vec![0],
            vec![true],
            1,
            1,
        );
        assert!(!game.rotate(0, Rotation::Down));
        assert_eq!(game.piece_at(0, 1), Piece::Horizontal);
        assert_eq!(game.moves(), 0);
    }

    #[test]
    fn reset_restores_the_initial_scroll() {
        let mut game = PipesGame::from_parts(
            3,
            vec![vec![Piece::Horizontal, Piece::Vertical, Piece::Vertical]],
            vec![1],
            vec![false],
            0,
            0,
        );
        let before = game.piece_at(0, 0);
        game.rotate(0, Rotation::Down);
        game.rotate(0, Rotation::Down);
        game.reset();
        assert_eq!(game.piece_at(0, 0), before);
        assert_eq!(game.moves(), 0);
    }

    #[test]
    fn water_fills_over_time_and_snaps_back_when_shortened() {
        let mut game = board(
            1,
            vec![vec![Piece::Horizontal], vec![Piece::Horizontal]],
            0,
            0,
        );
        assert_eq!(game.fill_progress(), 0.0);
        assert!(game.is_animating());
        game.advance(10.0);
        assert_eq!(game.fill_progress(), 2.0);
        assert!(!game.is_animating());

        // A one-row board with a vertical has no flow at all: the fill must
        // snap straight back rather than draining.
        let mut game = board(1, vec![vec![Piece::Vertical]], 0, 0);
        game.fill = 5.0;
        game.advance(0.016);
        assert_eq!(game.fill_progress(), 0.0);
    }
}
