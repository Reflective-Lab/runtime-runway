//! Contract tests for `runway-storage`. Asserts equivalence and shape parity
//! across backends. See `docs/superpowers/specs/2026-05-26-runway-storage-contract-tests-design.md`.

pub mod document;
pub mod embedding;
pub mod event;
pub mod harness;
pub mod object;
pub mod vector;

pub use harness::{ContractContext, SuiteReport};
