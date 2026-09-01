#![doc = include_str!("../README.md")]

mod game;
mod generator;
mod solver;
mod widget;

pub use game::{FlowStep, GameStatus, Piece, PipesGame, Rotation, Side};
pub use widget::{content_size, fit_cell_size, PipesWidget, DEFAULT_WATER_COLOR};
