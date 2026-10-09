#!/usr/bin/env python3
"""Interleave release binaries of the import scaling probe on a shared host.

Build/copy each revision's import_scaling_stress executable as old/main/fixed,
then run with --bin-dir and --output-dir. All dependencies are Python stdlib.
"""
import argparse
import csv
import json
import os
import re
import statistics
import subprocess
from pathlib import Path

SCENARIOS = [
    "overlap_snap", "overlap_partial", "overlap_mem", "snap_plus", "snap_mem",
    "detached", "mlist_batch", "text_stream", "batch",
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--fat", type=int, default=64)
    parser.add_argument("--scenarios", nargs="+", choices=SCENARIOS, default=SCENARIOS)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    with (args.output_dir / "raw.jsonl").open("w") as raw:
        for scenario in args.scenarios:
            for n in [8000, 32000, 64000]:
                for rep in range(3):
                    for version in ["old", "main", "fixed"]:
                        env = os.environ.copy()
                        env.update(CARGO_BUILD_JOBS="4", FAT="1", REPEATS="4")
                        env.pop("SPREAD", None)
                        env.pop("SEEDSYNC", None)
                        env.pop("NOSEED", None)
                        if scenario in SCENARIOS[:5]:
                            env["FAT"] = str(args.fat)
                        if scenario == "text_stream":
                            env["NOSEED"] = "1"
                        run = subprocess.run(
                            [str(args.bin_dir.resolve() / version), scenario, str(n), "3"],
                            env=env, capture_output=True, text=True,
                        )
                        load = subprocess.run(
                            ["uptime"], capture_output=True, text=True,
                        ).stdout.strip()
                        record = dict(
                            scenario=scenario, n=n, rep=rep, version=version,
                            load=load, stdout=run.stdout, stderr=run.stderr,
                            exit=run.returncode,
                        )
                        raw.write(json.dumps(record) + "\n")
                        raw.flush()
                        if run.returncode:
                            raise RuntimeError(record)
                        if scenario == "snap_mem":
                            samples = re.findall(
                                r"MEM kind=snap i=\d+ import_ms=([\d.]+) "
                                r"heap_kb=(\d+) delta_kb=(-?\d+)", run.stdout,
                            )
                            base = int(re.search(
                                r"MEM kind=snap_base heap_kb=(\d+)", run.stdout,
                            )[1])
                            ms = statistics.median(float(x[0]) for x in samples)
                            heap = int(samples[-1][1])
                            delta = heap - base
                        else:
                            ms = float(re.search(r"median_ms=([\d.]+)", run.stdout)[1])
                            heap = 0
                            match = re.search(r"heap_delta_kb=(\d+)", run.stdout)
                            delta = int(match[1]) if match else 0
                        rows.append(dict(
                            scenario=scenario, n=n, rep=rep, version=version,
                            ms=ms, heap_kb=heap, delta_kb=delta,
                        ))
                    print(f"{scenario} {n} rep {rep + 1}/3", flush=True)
    with (args.output_dir / "samples.csv").open("w") as output:
        writer = csv.DictWriter(output, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(rows)
    print("MEASUREMENT_COMPLETE", flush=True)


if __name__ == "__main__":
    main()
