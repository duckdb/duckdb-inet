#!/usr/bin/env python3
"""Compare two release-built inet extensions in isolated DuckDB processes."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time

import duckdb

QUERIES = {
    "parse_ipv4": "SELECT sum(hash(text4::INET)) FROM inputs",
    "parse_ipv6": "SELECT sum(hash(text6::INET)) FROM inputs",
    "format": "SELECT sum(length(ip::VARCHAR)) FROM addresses",
    "host": "SELECT sum(length(host(ip))) FROM addresses",
    "family": "SELECT sum(family(ip)) FROM addresses",
    "network": "SELECT sum(hash(network(ip))) FROM addresses",
    "offset": "SELECT sum(hash(ip + delta)) FROM addresses",
    "contains": "SELECT sum(ip <<= subnet) FROM addresses",
    "selected_network": (
        "SELECT sum(hash(network(ip))) FROM addresses WHERE i % 7 = 3"
    ),
    "html_escape": "SELECT sum(length(html_escape(text, quote))) FROM inputs",
    "html_unescape": "SELECT sum(length(html_unescape(escaped))) FROM addresses",
}


def worker(extension, rows):
    connection = duckdb.connect(
        config={
            "allow_unsigned_extensions": "true",
            "autoload_known_extensions": "false",
            "autoinstall_known_extensions": "false",
            "threads": "1",
        }
    )
    path = str(extension.resolve()).replace("'", "''")
    connection.execute(f"LOAD '{path}'")
    connection.execute(f"""
        CREATE TABLE inputs AS SELECT i,
            CASE WHEN i % 11 != 0 THEN
                '10.' || ((i >> 16) % 256)::VARCHAR || '.' ||
                ((i >> 8) % 256)::VARCHAR || '.' || (i % 256)::VARCHAR || '/24'
            END AS text4,
            CASE WHEN i % 11 != 0 THEN
                '2001:db8:' || to_hex(i % 65536) || '::' ||
                to_hex(i % 32768) || '/64'
            END AS text6,
            CASE WHEN i % 11 != 0 THEN
                '<a title="duck & ' || i::VARCHAR || '">hello</a>'
            END AS text,
            CASE WHEN i % 13 != 0 THEN i % 2 = 0 END AS quote
        FROM range({rows}) t(i)
        """)
    connection.execute("""
        CREATE TABLE addresses AS SELECT i, ip,
            CASE WHEN i % 13 != 0 THEN (i % 255 - 127)::HUGEINT END AS delta,
            CASE WHEN i % 17 != 0 THEN network(ip) END AS subnet,
            html_escape(text) AS escaped
        FROM (SELECT *, CASE WHEN i % 2 = 0 THEN text4 ELSE text6 END::INET AS ip
              FROM inputs)
        """)
    samples = {}
    for name, sql in QUERIES.items():
        expected = connection.execute(sql).fetchall()
        start = time.perf_counter()
        actual = connection.execute(sql).fetchall()
        elapsed = (time.perf_counter() - start) * 1000
        assert actual == expected, name
        samples[name] = {"ms": elapsed, "result": actual}
    connection.close()
    return samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--rows", type=int, default=1_000_000)
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.rows < 1 or args.rounds < 1:
        parser.error("rows and rounds must be positive")
    if args.worker:
        print(json.dumps(worker(args.worker, args.rows)))
        return
    if args.baseline is None or args.candidate is None:
        parser.error("--baseline and --candidate are required")

    variants = {
        "baseline": args.baseline.resolve(),
        "candidate": args.candidate.resolve(),
    }
    report = {
        "duckdb": duckdb.__version__,
        "rows": args.rows,
        "rounds": args.rounds,
        "threads": 1,
        "cores": os.cpu_count(),
        "load_start": os.getloadavg(),
        "extensions": {
            name: {
                "path": str(path),
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
            for name, path in variants.items()
        },
        "samples": {name: [] for name in variants},
    }
    for round_index in range(args.rounds):
        order = list(variants) if round_index % 2 == 0 else list(reversed(variants))
        for name in order:
            output = subprocess.check_output(
                [
                    sys.executable,
                    str(Path(__file__).resolve()),
                    "--worker",
                    str(variants[name]),
                    "--rows",
                    str(args.rows),
                ],
                text=True,
                timeout=180,
            )
            report["samples"][name].append(json.loads(output))
        for name in QUERIES:
            assert (
                report["samples"]["baseline"][-1][name]["result"]
                == report["samples"]["candidate"][-1][name]["result"]
            ), name
        print(f"Round {round_index + 1}/{args.rounds} complete", file=sys.stderr)
    report["load_end"] = os.getloadavg()
    print(
        "Query                 Baseline ms [min, max]    Candidate ms [min, max]   Change"
    )
    for query in QUERIES:
        summary = {}
        for name in variants:
            values = [sample[query]["ms"] for sample in report["samples"][name]]
            summary[name] = (statistics.median(values), min(values), max(values))
        baseline, candidate = summary["baseline"], summary["candidate"]
        change = (candidate[0] / baseline[0] - 1) * 100
        overlap = max(baseline[1], candidate[1]) <= min(baseline[2], candidate[2])
        noisy = any(
            (high - low) / median > 0.15 for median, low, high in summary.values()
        )
        caveat = " high variance" if noisy else " overlapping ranges" if overlap else ""
        print(
            f"{query:20} {baseline[0]:7.2f} [{baseline[1]:6.2f}, {baseline[2]:6.2f}]"
            f"   {candidate[0]:7.2f} [{candidate[1]:6.2f}, {candidate[2]:6.2f}]"
            f"   {change:+6.1f}%{caveat}"
        )
    print(
        f"Load: {report['load_start']} -> {report['load_end']}; {report['cores']} cores"
    )
    if args.output:
        args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
