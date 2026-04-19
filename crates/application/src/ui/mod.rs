//! TUI Module for Converge Application
//!
//! This module provides the terminal user interface for Converge,
//! allowing interactive job submission, monitoring, and context visualization.

pub mod app;
pub mod views;

pub use app::{App, run_app};
