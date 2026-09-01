//! The egui widget that renders and drives a [`PipesGame`]. This is the only
//! module allowed to touch `egui::Ui`/`Painter`.
//!
//! # Rendering convention (easy to get wrong, so it is called out here)
//!
//! A pipe is **genuinely hollow**: its two walls are drawn as two separate
//! lines offset from the centerline, and the space between them is never
//! painted over. Do not go back to the tempting shortcut of one wide casing
//! stroke with a narrower background-colored stroke punched through it: that
//! only looks hollow when the background is opaque, and embedding apps with
//! a translucent theme (a blurred backdrop, a live wallpaper) get a pale
//! smear instead of a bore.
//!
//! The corollary that is easy to get wrong: a corner's walls are two
//! **concentric arcs** (`radius +/- offset` around the same centre), not the
//! centerline arc shifted sideways. Concentric arcs land exactly on
//! `cell_center +/- offset` on both shared edges, which is where the
//! neighbouring cell's straight walls also land, so the pipework joins up
//! seamlessly. Any other offsetting scheme leaves visible notches at every
//! corner.

use std::f32::consts::{PI, TAU};

use egui::{
    emath::GuiRounding, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, Widget,
};

use crate::game::{FlowStep, GameStatus, Piece, PipesGame, Rotation, Side};

/// The default color of the water, a mid cyan-blue that stays legible on
/// both a light and a dark board. Override with [`PipesWidget::water_color`].
pub const DEFAULT_WATER_COLOR: Color32 = Color32::from_rgb(0x2E, 0x9E, 0xD9);

/// Inner (water-carrying) diameter of a pipe, as a fraction of the cell size.
const BORE_WIDTH: f32 = 0.30;
/// Thickness of each of a pipe's two walls, as a fraction of the cell size.
const WALL_THICKNESS: f32 = 0.075;
/// Line segments used to approximate a corner's quarter arc.
const ARC_STEPS: usize = 10;
/// Seconds a column takes to slide into place after a scroll.
const SCROLL_ANIM_SECS: f32 = 0.12;
/// Scroll delta (in points) that counts as one row of wheel movement. egui
/// delivers roughly 50 points per wheel notch by default.
const SCROLL_NOTCH: f32 = 45.0;

/// The footprint [`PipesWidget`] occupies for `game` at a given `cell_size`.
/// One extra cell of width covers the inlet and outlet stubs, half a cell on
/// each side of the board.
pub fn content_size(game: &PipesGame, cell_size: f32) -> Vec2 {
    Vec2::new(game.columns() as f32 + 1.0, game.rows() as f32) * cell_size
}

/// The cell size that fits `game` into `available` space, which is the same
/// formula [`PipesWidget`] uses when no explicit `cell_size` is set. Lets a
/// caller pre-compute the fit instead of duplicating it.
pub fn fit_cell_size(game: &PipesGame, available: Vec2) -> f32 {
    let by_width = available.x / (game.columns() as f32 + 1.0);
    let by_height = available.y / game.rows() as f32;
    by_width.min(by_height).max(4.0)
}

/// An egui widget that renders an interactive pipe board.
///
/// Click an unlocked column to scroll it down one row (the bottom piece
/// wraps around to the top); the mouse wheel over a column scrolls it either
/// way. Individual pieces never rotate. Locked columns are drawn darker and
/// ignore both.
///
/// ```ignore
/// ui.add(egui_pipes::PipesWidget::new(&mut game));
/// ```
pub struct PipesWidget<'a> {
    game: &'a mut PipesGame,
    cell_size: Option<f32>,
    water_color: Color32,
    win_message: Option<String>,
    interactive: bool,
    scrolled: Option<&'a mut bool>,
}

impl<'a> PipesWidget<'a> {
    pub fn new(game: &'a mut PipesGame) -> Self {
        Self {
            game,
            cell_size: None,
            water_color: DEFAULT_WATER_COLOR,
            win_message: None,
            interactive: true,
            scrolled: None,
        }
    }

    /// Override the size (in logical pixels) of each grid cell. When unset,
    /// the cell size is computed to fill the parent container.
    pub fn cell_size(mut self, size: f32) -> Self {
        self.cell_size = Some(size);
        self
    }

    /// The color the pipes fill with. Defaults to [`DEFAULT_WATER_COLOR`].
    pub fn water_color(mut self, color: Color32) -> Self {
        self.water_color = color;
        self
    }

    /// Message shown in the banner drawn over the board once the water
    /// reaches the outlet. Defaults to `"Connected!"`.
    pub fn win_message(mut self, message: impl Into<String>) -> Self {
        self.win_message = Some(message.into());
        self
    }

    /// Whether the widget responds to input at all. Set to `false` to render
    /// the board read-only. Defaults to `true`.
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Set to `true` when this frame's input scrolled a column with the
    /// mouse wheel rather than a click. Lets an embedding app notice that
    /// the player found the wheel control, which is otherwise invisible.
    pub fn scrolled(mut self, flag: &'a mut bool) -> Self {
        self.scrolled = Some(flag);
        self
    }
}

/// The midpoint of one of a cell's edges: where a pipe crosses into the
/// neighbouring cell.
fn side_point(rect: Rect, side: Side) -> Pos2 {
    match side {
        Side::Left => Pos2::new(rect.min.x, rect.center().y),
        Side::Right => Pos2::new(rect.max.x, rect.center().y),
        Side::Up => Pos2::new(rect.center().x, rect.min.y),
        Side::Down => Pos2::new(rect.center().x, rect.max.y),
    }
}

/// The cell corner shared by two perpendicular edges: the centre a corner
/// piece's quarter arc turns around.
fn shared_corner(rect: Rect, a: Side, b: Side) -> Pos2 {
    let x = if a == Side::Left || b == Side::Left {
        rect.min.x
    } else {
        rect.max.x
    };
    let y = if a == Side::Up || b == Side::Up {
        rect.min.y
    } else {
        rect.max.y
    };
    Pos2::new(x, y)
}

/// The route through one cell: the line the water runs along, and the shape
/// both walls are derived from.
#[derive(Clone, Copy)]
enum Path {
    Straight {
        from: Pos2,
        to: Pos2,
    },
    Arc {
        center: Pos2,
        radius: f32,
        start: f32,
        sweep: f32,
    },
}

impl Path {
    /// The route through `rect`, oriented so it starts at the `entry` edge:
    /// the water's direction of travel, which is what the fill animation
    /// advances along.
    fn new(rect: Rect, piece: Piece, entry: Side) -> Self {
        let [a, b] = piece.sides();
        let (from, to) = if a == entry { (a, b) } else { (b, a) };

        if !piece.is_corner() {
            return Self::Straight {
                from: side_point(rect, from),
                to: side_point(rect, to),
            };
        }

        let center = shared_corner(rect, from, to);
        let offset = side_point(rect, from) - center;
        let start = offset.angle();
        let mut sweep = (side_point(rect, to) - center).angle() - start;
        // Always take the short way round: a corner turns 90 degrees, never 270.
        while sweep > PI {
            sweep -= TAU;
        }
        while sweep < -PI {
            sweep += TAU;
        }

        Self::Arc {
            center,
            radius: offset.length(),
            start,
            sweep,
        }
    }

    /// Samples the route pushed sideways by `offset`: `0.0` is the
    /// centerline the water runs along, `+/-` a wall offset gives the two
    /// casing walls. A corner is offset as a **concentric arc**, never as a
    /// shifted chord; see the module docs for why that matters.
    fn points(self, offset: f32) -> Vec<Pos2> {
        match self {
            Self::Straight { from, to } => {
                let push = (to - from).normalized().rot90() * offset;
                vec![from + push, to + push]
            }
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                // The arc's own outward normal points away from the centre,
                // so a positive offset is "further from the corner" on one
                // side and the sweep's sign decides which. Either way the two
                // signs give the two walls.
                let signed = if sweep < 0.0 { -offset } else { offset };
                (0..=ARC_STEPS)
                    .map(|i| {
                        let angle = start + sweep * i as f32 / ARC_STEPS as f32;
                        center + Vec2::angled(angle) * (radius + signed)
                    })
                    .collect()
            }
        }
    }
}

/// The leading `t` (0..=1) of a polyline by arc length: the wet part of a
/// partially filled pipe.
fn polyline_prefix(points: &[Pos2], t: f32) -> Vec<Pos2> {
    if t <= 0.0 || points.len() < 2 {
        return Vec::new();
    }
    if t >= 1.0 {
        return points.to_vec();
    }

    let total: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut budget = total * t;
    let mut out = vec![points[0]];
    for w in points.windows(2) {
        let length = w[0].distance(w[1]);
        if length <= f32::EPSILON {
            continue;
        }
        if budget >= length {
            out.push(w[1]);
            budget -= length;
        } else {
            out.push(w[0] + (w[1] - w[0]) * (budget / length));
            break;
        }
    }
    out
}

/// How a length of pipe is painted. `wall_offset` is the distance from the
/// centerline to each wall's own centerline, so the walls' inner edges sit
/// exactly `bore_width / 2` out and the water meets them flush.
#[derive(Clone, Copy)]
struct PipeStyle {
    casing: Color32,
    wall_offset: f32,
    wall_thickness: f32,
    bore_width: f32,
    water: Color32,
}

impl PipeStyle {
    fn new(cell: f32, casing: Color32, water: Color32) -> Self {
        let bore_width = cell * BORE_WIDTH;
        let wall_thickness = cell * WALL_THICKNESS;
        Self {
            casing,
            wall_offset: (bore_width + wall_thickness) * 0.5,
            wall_thickness,
            bore_width,
            water,
        }
    }
}

/// Whatever water has reached this stretch, then the two walls over it.
fn draw_pipe(painter: &egui::Painter, path: Path, style: PipeStyle, fill: f32) {
    if fill > 0.0 {
        let wet = polyline_prefix(&path.points(0.0), fill);
        if wet.len() >= 2 {
            painter.add(Shape::line(wet, Stroke::new(style.bore_width, style.water)));
        }
    }

    let wall = Stroke::new(style.wall_thickness, style.casing);
    painter.add(Shape::line(path.points(style.wall_offset), wall));
    painter.add(Shape::line(path.points(-style.wall_offset), wall));
}

/// A small padlock, drawn behind a locked column's pipes.
fn draw_padlock(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let body = Rect::from_center_size(
        center + Vec2::new(0.0, size * 0.18),
        Vec2::new(size * 0.72, size * 0.56),
    );
    painter.rect_filled(
        body,
        CornerRadius::same((size * 0.12).max(1.0) as u8),
        color,
    );

    // Shackle: the upper half of a circle sitting on top of the body. With
    // y pointing down, PI..TAU is the half above the centre.
    let radius = size * 0.24;
    let hinge = center - Vec2::new(0.0, size * 0.10);
    let shackle: Vec<Pos2> = (0..=12)
        .map(|i| hinge + Vec2::angled(PI + PI * i as f32 / 12.0) * radius)
        .collect();
    painter.add(Shape::line(shackle, Stroke::new(size * 0.13, color)));
}

impl Widget for PipesWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            game,
            cell_size,
            water_color,
            win_message,
            interactive,
            mut scrolled,
        } = self;

        let columns = game.columns();
        let rows = game.rows();
        let cell = cell_size.unwrap_or_else(|| fit_cell_size(game, ui.available_size()));
        let total_size = content_size(game, cell);

        let sense = if interactive {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (response, painter) = ui.allocate_painter(total_size, sense);
        let origin = response.rect.min;
        // Half a cell of margin on the left holds the inlet stub.
        let board_left = origin.x + cell * 0.5;

        let cell_rect = |col: usize, row: usize| -> Rect {
            Rect::from_min_size(
                Pos2::new(board_left + col as f32 * cell, origin.y + row as f32 * cell),
                Vec2::splat(cell),
            )
        };
        let column_rect = |col: usize| -> Rect {
            Rect::from_min_size(
                Pos2::new(board_left + col as f32 * cell, origin.y),
                Vec2::new(cell, rows as f32 * cell),
            )
        };
        let col_at = |pos: Pos2| -> Option<usize> {
            let x = pos.x - board_left;
            let y = pos.y - origin.y;
            if x < 0.0 || y < 0.0 || y >= rows as f32 * cell {
                return None;
            }
            let col = (x / cell).floor() as usize;
            (col < columns).then_some(col)
        };

        // ── Animation bookkeeping ───────────────────────────────────────
        // One entry per column: how many rows the column is drawn away from
        // where it logically sits, decaying to zero. Kept in egui's temp
        // memory so `PipesGame` stays free of presentation state.
        let anim_id = response.id.with("pipes_column_slide");
        let mut slide: Vec<f32> = ui.ctx().data(|d| d.get_temp(anim_id)).unwrap_or_default();
        slide.resize(columns, 0.0);

        let dt = ui.input(|i| i.stable_dt);
        game.advance(dt);
        let decay = dt / SCROLL_ANIM_SECS;
        for offset in &mut slide {
            *offset = if *offset > 0.0 {
                (*offset - decay).max(0.0)
            } else {
                (*offset + decay).min(0.0)
            };
        }

        // ── Input ───────────────────────────────────────────────────────
        let hovered = response.hover_pos().and_then(col_at);
        if interactive {
            if response.clicked() {
                if let Some(col) = response.interact_pointer_pos().and_then(col_at) {
                    if game.rotate(col, Rotation::Down) {
                        // The column starts one row high and slides down.
                        slide[col] = -1.0;
                    }
                }
            }

            if let Some(col) = hovered {
                if game.is_locked(col) {
                    ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
                } else {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }

                // Wheel scrolling, accumulated so one notch is one row
                // whatever the device's delta granularity. Consumed here so
                // an enclosing scroll area or scene doesn't also act on it.
                let acc_id = response.id.with("pipes_scroll_accumulator");
                let raw = ui.input_mut(|i| {
                    let delta = i.smooth_scroll_delta.y;
                    if delta != 0.0 {
                        i.smooth_scroll_delta.y = 0.0;
                    }
                    delta
                });
                // egui spreads one wheel notch over a few frames, so the
                // accumulator is reset whenever the wheel goes quiet rather
                // than carrying a residue that would eventually drift into
                // an extra row.
                let mut acc: f32 = if raw == 0.0 {
                    0.0
                } else {
                    ui.ctx().data(|d| d.get_temp(acc_id)).unwrap_or(0.0) + raw
                };
                while acc.abs() >= SCROLL_NOTCH {
                    // Wheel up moves the column up: the pieces follow the
                    // wheel rather than the usual "content scrolls the other
                    // way" convention, since the player is turning a reel.
                    let (direction, step) = if acc > 0.0 {
                        (Rotation::Up, -SCROLL_NOTCH)
                    } else {
                        (Rotation::Down, SCROLL_NOTCH)
                    };
                    acc += step;
                    if game.rotate(col, direction) {
                        slide[col] = if direction == Rotation::Down {
                            -1.0
                        } else {
                            1.0
                        };
                        if let Some(flag) = scrolled.as_deref_mut() {
                            *flag = true;
                        }
                    }
                }
                ui.ctx().data_mut(|d| d.insert_temp(acc_id, acc));
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(anim_id, slide.clone()));

        // ── Water lookup ────────────────────────────────────────────────
        // How full each visited cell is, and which way the water runs
        // through it. The flow never revisits a cell (it only ever moves
        // right, or up/down within one column before turning right), so a
        // flat array is enough.
        let mut wetness: Vec<Option<(Side, Side, f32)>> = vec![None; columns * rows];
        let fill = game.fill_progress();
        for (i, step) in game.flow().iter().enumerate() {
            let FlowStep {
                col,
                row,
                entry,
                exit,
            } = *step;
            let amount = (fill - i as f32).clamp(0.0, 1.0);
            wetness[col * rows + row] = Some((entry, exit, amount));
        }

        // ── Painting ────────────────────────────────────────────────────
        let visuals = ui.visuals();
        let ppi = painter.ctx().pixels_per_point();

        let open_bg = visuals.extreme_bg_color;
        let locked_bg = visuals.widgets.noninteractive.bg_fill;
        let open_casing = if visuals.dark_mode {
            Color32::from_gray(0x86)
        } else {
            Color32::from_gray(0x77)
        };
        let locked_casing = if visuals.dark_mode {
            Color32::from_gray(0x54)
        } else {
            Color32::from_gray(0xA6)
        };
        let grid_stroke = Stroke::new(0.5, visuals.widgets.noninteractive.bg_stroke.color);

        for col in 0..columns {
            let is_locked = game.is_locked(col);
            let bg = if is_locked { locked_bg } else { open_bg };
            let style = PipeStyle::new(
                cell,
                if is_locked {
                    locked_casing
                } else {
                    open_casing
                },
                water_color,
            );
            let rect = column_rect(col);

            painter.rect_filled(rect.round_to_pixels(ppi), 0.0, bg);
            for row in 0..rows {
                painter.rect_stroke(
                    cell_rect(col, row).round_to_pixels(ppi),
                    0.0,
                    grid_stroke,
                    StrokeKind::Inside,
                );
            }

            if is_locked {
                draw_padlock(
                    &painter,
                    rect.center(),
                    cell * 1.1,
                    visuals.widgets.noninteractive.bg_stroke.color,
                );
                painter.rect_stroke(
                    rect.round_to_pixels(ppi),
                    0.0,
                    Stroke::new(1.5, visuals.widgets.noninteractive.fg_stroke.color),
                    StrokeKind::Inside,
                );
            } else if hovered == Some(col) && interactive && game.status() != GameStatus::Won {
                painter.rect_filled(rect, 0.0, Color32::from_white_alpha(12));
            }

            // Pipes, clipped to the column so a mid-scroll slide can show
            // the wrapping piece coming in from the edge.
            let shift = Vec2::new(0.0, slide[col] * cell);
            let column_painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            for row in -1..=(rows as isize) {
                let wrapped = row.rem_euclid(rows as isize) as usize;
                let piece = game.piece_at(col, wrapped);
                let draw_rect = Rect::from_min_size(
                    Pos2::new(board_left + col as f32 * cell, origin.y + row as f32 * cell),
                    Vec2::splat(cell),
                )
                .translate(shift);

                // Only the row actually on screen carries water; the extra
                // copies above and below exist purely for the slide.
                let wet = (row == wrapped as isize)
                    .then(|| wetness[col * rows + wrapped])
                    .flatten();
                let (entry, amount) = match wet {
                    Some((entry, _, amount)) => (entry, amount),
                    None => (piece.sides()[0], 0.0),
                };
                draw_pipe(
                    &column_painter,
                    Path::new(draw_rect, piece, entry),
                    style,
                    amount,
                );
            }
        }

        // ── Inlet and outlet stubs ──────────────────────────────────────
        let source = Pos2::new(origin.x, origin.y + (game.start_row() as f32 + 0.5) * cell);
        let inlet = Path::Straight {
            from: source,
            to: side_point(cell_rect(0, game.start_row()), Side::Left),
        };
        let stub_style = PipeStyle::new(cell, open_casing, water_color);
        // The source is always running, even when the first piece walls it
        // off: the water simply stops against that wall.
        draw_pipe(&painter, inlet, stub_style, 1.0);

        let goal = Pos2::new(
            origin.x + total_size.x,
            origin.y + (game.end_row() as f32 + 0.5) * cell,
        );
        let outlet = Path::Straight {
            from: side_point(cell_rect(columns - 1, game.end_row()), Side::Right),
            to: goal,
        };
        let outlet_fill = if game.reaches_goal() && !game.is_animating() {
            1.0
        } else {
            0.0
        };
        draw_pipe(&painter, outlet, stub_style, outlet_fill);

        // Source and target markers, sitting over the stubs' outer ends.
        painter.circle_filled(source, cell * 0.2, water_color);
        painter.circle_stroke(goal, cell * 0.2, Stroke::new(cell * 0.08, water_color));
        if outlet_fill > 0.0 {
            painter.circle_filled(goal, cell * 0.2, water_color);
        }

        // ── Win banner ──────────────────────────────────────────────────
        if game.status() == GameStatus::Won && !game.is_animating() {
            let message = win_message.unwrap_or_else(|| "Connected!".to_owned());
            let font = FontId::proportional((cell * 0.6).clamp(14.0, 40.0));
            let galley = painter.layout_no_wrap(
                message,
                font,
                visuals.widgets.noninteractive.fg_stroke.color,
            );
            // Hugging the top edge rather than centred: the completed run of
            // water is the reward for solving the board, and a banner in the
            // middle of it covers exactly the part worth looking at.
            let size = galley.size() + Vec2::new(cell, cell * 0.6);
            let banner = Rect::from_center_size(
                Pos2::new(
                    response.rect.center().x,
                    response.rect.min.y + size.y * 0.5 + cell * 0.15,
                ),
                size,
            );
            painter.rect_filled(banner, CornerRadius::same(6), visuals.window_fill);
            painter.rect_stroke(
                banner,
                CornerRadius::same(6),
                Stroke::new(1.0, water_color),
                StrokeKind::Inside,
            );
            painter.galley(
                banner.center() - galley.size() * 0.5,
                galley,
                Color32::PLACEHOLDER,
            );
        }

        if game.is_animating() || slide.iter().any(|offset| *offset != 0.0) {
            ui.ctx().request_repaint();
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::splat(10.0))
    }

    #[test]
    fn straight_centerline_runs_the_way_the_water_travels() {
        let points = Path::new(rect(), Piece::Horizontal, Side::Left).points(0.0);
        assert_eq!(points, vec![Pos2::new(0.0, 5.0), Pos2::new(10.0, 5.0)]);

        let reversed = Path::new(rect(), Piece::Horizontal, Side::Right).points(0.0);
        assert_eq!(reversed, vec![Pos2::new(10.0, 5.0), Pos2::new(0.0, 5.0)]);
    }

    #[test]
    fn corner_centerline_arcs_between_its_two_edges() {
        let points = Path::new(rect(), Piece::LeftDown, Side::Left).points(0.0);
        assert_eq!(points.len(), ARC_STEPS + 1);
        assert!(points[0].distance(Pos2::new(0.0, 5.0)) < 0.01);
        assert!(points[ARC_STEPS].distance(Pos2::new(5.0, 10.0)) < 0.01);
        // Every sample sits on the quarter circle centred on the cell's
        // bottom-left corner.
        for point in &points {
            assert!((point.distance(Pos2::new(0.0, 10.0)) - 5.0).abs() < 0.01);
        }
    }

    /// The join that breaks if a corner's walls are ever offset as a shifted
    /// chord instead of a concentric arc: both walls have to arrive on the
    /// shared edge at exactly the two points a straight's walls arrive at.
    #[test]
    fn corner_walls_meet_a_straights_walls_on_the_shared_edge() {
        let offset = 1.5;
        let corner = Path::new(rect(), Piece::LeftDown, Side::Left);
        let straight = Path::new(rect(), Piece::Horizontal, Side::Left);

        for side in [offset, -offset] {
            let corner_start = *corner.points(side).first().unwrap();
            let straight_start = *straight.points(side).first().unwrap();
            assert!(
                corner_start.distance(straight_start) < 0.01,
                "corner wall enters at {corner_start:?}, straight wall at {straight_start:?}"
            );
        }

        // The far end lands on the bottom edge, mirrored about the cell's
        // vertical centerline.
        let ends: Vec<Pos2> = [offset, -offset]
            .into_iter()
            .map(|side| *corner.points(side).last().unwrap())
            .collect();
        for end in &ends {
            assert!((end.y - 10.0).abs() < 0.01, "wall does not reach the edge");
        }
        assert!((ends[0].x + ends[1].x - 10.0).abs() < 0.01);
    }

    #[test]
    fn polyline_prefix_cuts_by_arc_length() {
        let points = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)];
        assert!(polyline_prefix(&points, 0.0).is_empty());
        assert_eq!(polyline_prefix(&points, 1.0).len(), 2);
        let half = polyline_prefix(&points, 0.5);
        assert_eq!(half.len(), 2);
        assert!((half[1].x - 5.0).abs() < 0.01);
    }

    #[test]
    fn content_size_reserves_room_for_the_stubs() {
        let game = PipesGame::random(6, 5, 0.5, 1);
        assert_eq!(content_size(&game, 10.0), Vec2::new(70.0, 50.0));
        assert_eq!(fit_cell_size(&game, Vec2::new(70.0, 50.0)), 10.0);
    }
}
