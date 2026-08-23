//! Portly library surface: Elm-style TUI core plus platform collectors.
//!
//! The `portly` binary is a thin shell around these modules; keeping them in
//! a library makes benchmarks, integration tests, and scripting reuse trivial.

pub mod collectors;
pub mod config;
#[cfg(feature = "docker")]
pub mod docker;
pub mod health;
pub mod json;
pub mod logs;
pub mod model;
pub mod view;
