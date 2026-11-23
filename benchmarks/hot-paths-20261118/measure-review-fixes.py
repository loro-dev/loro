import itertools
import json
import os
import pathlib
import statistics
import subprocess
import sys

root = pathlib.Path(sys.argv[1])
previous = root.parent / "pr1314-review" / "binaries"
os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})


def binary(variant, scalar):
    directory = root / "binaries" if variant == "fixed" else previous
    return str(directory / (variant + ("-scalars" if scalar else "")))


def measure(name, scenarios, counts, variants, scalar, orders=None):
    samples = {}
    orders = orders or list(itertools.permutations(variants))
    with (root / f"{name}-measurements.jsonl").open("x") as output:
        for scenario in scenarios:
            for count in counts:
                for iteration, order in enumerate([variants] + orders):
                    for variant in order:
                        command = [binary(variant, scalar), scenario, str(count)]
                        result = subprocess.run(command, capture_output=True, text=True, timeout=90)
                        output.write(json.dumps(dict(
                            scenario=scenario, count=count, variant=variant, iteration=iteration,
                            returncode=result.returncode, stdout=result.stdout, stderr=result.stderr
                        )) + "\n")
                        output.flush()
                        result.check_returncode()
                        fields = result.stdout.strip().split(",")
                        assert fields[:2] == [scenario, str(count)]
                        if iteration:
                            metrics = ["elapsed_ns"] if scalar else ["edit_ns", "undo_ns", "redo_ns"]
                            values = samples.setdefault(f"{scenario}/{count}", {}).setdefault(
                                variant, {metric: [] for metric in metrics})
                            for metric, value in zip(metrics, fields[2:]):
                                values[metric].append(int(value))
                summary = {
                    variant: {metric: round(statistics.median(values) / 1e6, 6)
                              for metric, values in measurements.items()}
                    for variant, measurements in samples[f"{scenario}/{count}"].items()
                }
                print(name, scenario, count, json.dumps(summary), flush=True)
                (root / f"{name}-samples.json").write_text(json.dumps(samples, indent=2))


measure("grouped", ["contiguous", "map_tail", "two_texts", "list", "delete", "separated"],
        [1000, 5000, 20000], ["prestack", "head", "fixed"], False)
variants = ["prestack", "main", "head", "fixed"]
rotations = [variants[offset:] + variants[:offset] for offset in range(len(variants))]
measure("scalar", ["list", "movable"], [0, 1, 8, 128], variants, True,
        rotations + [list(reversed(order)) for order in rotations])
measure("delta", ["delta"], [1000, 5000, 20000], ["head", "fixed"], True,
        [["head", "fixed"], ["fixed", "head"]] * 3)
print("MEASUREMENTS_COMPLETE", flush=True)
