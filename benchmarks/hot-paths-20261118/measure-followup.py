import argparse
import hashlib
import itertools
import json
import os
import pathlib
import random
import statistics
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=pathlib.Path)
args = parser.parse_args()
root = args.directory.resolve()
variants = ["prestack", "pr", "narrow", "borrowed"]
orders = list(itertools.permutations(variants))
random.Random(1315).shuffle(orders)
os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
samples = {}
with (root / "final-measurements.jsonl").open("x") as output:
    for iteration, order in enumerate([variants] + orders):
        workloads = [
            ("hot", scenario)
            for scenario in ("text", "map", "undo", "jsonpath", "root_lookup", "child_lookup")
        ] + [("followup", scenario) for scenario in ("batched", "grouped")]
        for driver, scenario in workloads:
            for variant in order:
                command = [str(root / "binaries" / f"{variant}-{driver}"), scenario]
                if driver == "followup":
                    command.append("20000")
                result = subprocess.run(command, capture_output=True, text=True, timeout=90)
                row = dict(
                    iteration=iteration,
                    driver=driver,
                    scenario=scenario,
                    variant=variant,
                    returncode=result.returncode,
                    stdout=result.stdout,
                    stderr=result.stderr,
                )
                output.write(json.dumps(row) + "\n")
                output.flush()
                result.check_returncode()
                fields = result.stdout.strip().split(",")
                assert fields[0] == scenario
                if iteration:
                    edit_ns = int(fields[1] if driver == "hot" else fields[2])
                    undo_ns = 0 if driver == "hot" else int(fields[3])
                    values = samples.setdefault(f"{driver}/{scenario}", {}).setdefault(
                        variant, {"edit_ns": [], "undo_ns": [], "total_ns": []}
                    )
                    values["edit_ns"].append(edit_ns)
                    values["undo_ns"].append(undo_ns)
                    values["total_ns"].append(edit_ns + undo_ns)
        print(f"FINAL ROUND {iteration} COMPLETE", flush=True)
for scenario, variant_samples in samples.items():
    for values in variant_samples.values():
        values["medians"] = {
            metric: statistics.median(measurements)
            for metric, measurements in values.items()
        }
    print(scenario, {
        variant: {metric: round(value / 1e6, 3) for metric, value in values["medians"].items()}
        for variant, values in variant_samples.items()
    }, flush=True)
(root / "final-summary.json").write_text(json.dumps(samples, indent=2))
(root / "final-binary-sha256.json").write_text(json.dumps({
    binary.name: hashlib.sha256(binary.read_bytes()).hexdigest()
    for binary in (root / "binaries").iterdir()
}, indent=2))
