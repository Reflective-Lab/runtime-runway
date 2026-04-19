#!/usr/bin/env python3
"""Extract Criterion benchmark baselines to JSON and markdown.

Parses Criterion's JSON output from target/criterion/*/base/benchmark.json,
extracts key percentiles (p50, p95, p99) and statistics (mean, stddev),
and writes formatted summaries for trend tracking and regression detection.

Output files:
  - kb/Baselines/latest-baseline.json: Structured data (timestamp, run_id, benchmarks)
  - kb/Baselines/latest-summary.md: Human-readable markdown table
  - kb/Baselines/trends.csv: Appended to for historical tracking (date,benchmark,p50_us,...)
"""

import json
import sys
from pathlib import Path
from datetime import datetime
from typing import Optional, Dict, List, Any


def load_criterion_json(bench_name: str) -> Optional[Dict[str, Any]]:
    """Load a single Criterion benchmark JSON file.

    Args:
        bench_name: Benchmark name (e.g., 'engine_single_cycle')

    Returns:
        Parsed JSON dict, or None if file not found or invalid.
    """
    criterion_path = Path("target/criterion") / bench_name / "base" / "estimates.json"

    if not criterion_path.exists():
        return None

    try:
        with open(criterion_path) as f:
            return json.load(f)
    except Exception as e:
        print(f"⚠ Failed to load {criterion_path}: {e}", file=sys.stderr)
        return None


def extract_stats(data: Dict[str, Any]) -> Optional[Dict[str, float]]:
    """Extract p50, p95, p99, mean, stddev from Criterion estimates.json.

    Criterion stores timing data in nanoseconds under mean.point_estimate,
    median.point_estimate, and std_dev.point_estimate. We convert to microseconds.

    Args:
        data: Parsed Criterion estimates.json

    Returns:
        Dict with keys: p50_us, p95_us, p99_us, mean_us, std_dev_us (all floats)
        or None if data structure is unexpected.
    """
    try:
        # Get mean and median (p50 approximation)
        mean_data = data.get("mean", {})
        median_data = data.get("median", {})
        std_dev_data = data.get("std_dev", {})

        mean_ns = mean_data.get("point_estimate")
        median_ns = median_data.get("point_estimate")
        std_dev_ns = std_dev_data.get("point_estimate")

        if mean_ns is None or std_dev_ns is None:
            return None

        # Convert from nanoseconds to microseconds
        mean_us = mean_ns / 1000.0
        median_us = median_ns / 1000.0 if median_ns else mean_us
        std_dev_us = std_dev_ns / 1000.0

        # Estimate percentiles using normal distribution approximation
        # p50 = median, p95 ≈ mean + 1.96*stddev, p99 ≈ mean + 2.576*stddev
        p50_us = median_us
        p95_us = mean_us + (1.96 * std_dev_us)
        p99_us = mean_us + (2.576 * std_dev_us)

        return {
            "p50_us": round(p50_us, 2),
            "p95_us": round(p95_us, 2),
            "p99_us": round(p99_us, 2),
            "mean_us": round(mean_us, 2),
            "std_dev_us": round(std_dev_us, 2),
        }
    except Exception as e:
        print(f"⚠ Failed to extract stats: {e}", file=sys.stderr)
        return None


def main():
    """Main entry point."""
    now = datetime.utcnow()
    timestamp = now.isoformat() + "Z"
    run_id = now.strftime("%Y%m%d-%H%M%S")

    # Benchmarks to extract
    benchmarks = [
        "engine_single_cycle",
        "engine_multi_suggestor/suggestors/1",
        "engine_multi_suggestor/suggestors/5",
        "engine_multi_suggestor/suggestors/20",
        "engine_budget_pressure_near_ceiling",
        "engine_large_context_1000_facts",
    ]

    results = {}
    baseline_data = {
        "timestamp": timestamp,
        "run_id": run_id,
        "benchmarks": {}
    }

    for bench_name in benchmarks:
        data = load_criterion_json(bench_name)
        if data:
            stats = extract_stats(data)
            if stats:
                results[bench_name] = stats
                baseline_data["benchmarks"][bench_name] = stats

    if not results:
        print("✗ No benchmark data found. Have you run `cargo bench`?", file=sys.stderr)
        sys.exit(1)

    # Ensure output directory exists
    baselines_dir = Path("kb/Baselines")
    baselines_dir.mkdir(parents=True, exist_ok=True)

    # Write JSON baseline
    baseline_file = baselines_dir / "latest-baseline.json"
    with open(baseline_file, "w") as f:
        json.dump(baseline_data, f, indent=2)
    print(f"✓ Baseline JSON: {baseline_file}")

    # Write markdown summary
    summary_file = baselines_dir / "latest-summary.md"
    with open(summary_file, "w") as f:
        f.write(f"# Benchmark Baseline Summary\n\n")
        f.write(f"**Run:** {run_id} ({timestamp})\n\n")
        f.write("| Benchmark | p50 (µs) | p95 (µs) | p99 (µs) | Mean (µs) | StdDev (µs) |\n")
        f.write("|-----------|----------|----------|----------|-----------|-------------|\n")
        for bench_name in sorted(results.keys()):
            stats = results[bench_name]
            f.write(
                f"| {bench_name} | "
                f"{stats['p50_us']:.2f} | "
                f"{stats['p95_us']:.2f} | "
                f"{stats['p99_us']:.2f} | "
                f"{stats['mean_us']:.2f} | "
                f"{stats['std_dev_us']:.2f} |\n"
            )
    print(f"✓ Baseline summary: {summary_file}")

    # Append to trends.csv
    trends_file = baselines_dir / "trends.csv"

    # Write header if file doesn't exist
    if not trends_file.exists():
        with open(trends_file, "w") as f:
            f.write("date,run_id,benchmark,p50_us,p95_us,p99_us,mean_us,std_dev_us\n")

    # Append new data
    with open(trends_file, "a") as f:
        for bench_name in sorted(results.keys()):
            stats = results[bench_name]
            f.write(
                f"{now.strftime('%Y-%m-%d')},{run_id},{bench_name},"
                f"{stats['p50_us']},{stats['p95_us']},{stats['p99_us']},"
                f"{stats['mean_us']},{stats['std_dev_us']}\n"
            )
    print(f"✓ Trends appended: {trends_file}")

    print(f"\n✓ Extraction complete: {len(results)} benchmarks processed")
    sys.exit(0)


if __name__ == "__main__":
    main()
