// Copyright 2024-2026 Reflective Labs

//! Contract stress test runner.
//!
//! Run with: cargo run --example contract_stress --no-default-features --features ndarray

use converge_llm::contract_stress::run_output_stress_tests;

fn main() {
    println!("Running output contract stress tests...\n");

    let report = run_output_stress_tests();

    println!("{}", report.summary());

    if report.all_passed() {
        println!("\n✓ All contracts behaved as expected.");
    } else {
        println!("\n⚠ Some contracts need adjustment:");

        if !report.contract_too_weak.is_empty() {
            println!("\n  CONTRACTS TOO WEAK (need tightening):");
            for case in &report.contract_too_weak {
                println!("    - {} ({})", case.name, case.id);
            }
        }

        if !report.contract_too_strict.is_empty() {
            println!("\n  CONTRACTS TOO STRICT (need loosening):");
            for case in &report.contract_too_strict {
                println!("    - {} ({})", case.name, case.id);
            }
        }
    }

    // Exit with error if contracts need adjustment
    std::process::exit(if report.all_passed() { 0 } else { 1 });
}
