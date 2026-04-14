//! Guan Dan game server — MVP: table lifecycle, seq log, nextstate long-poll.

pub mod api;
pub mod domain;
pub mod error;
pub mod game;
pub mod prompt;
pub mod simulation;
pub mod store;
pub mod strategy;
pub mod web_assets;

pub use error::AppError;
