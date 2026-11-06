"""Measure the compiled-finalization change using frozen, unchanged workloads."""

import csv
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
REPO = Path.cwd()
sys.path.insert(0, str(REPO / "scripts"))
from compare_products import percentile, validate_rows, write_csv


def now():
    return datetime.now(timezone.utc).isoformat()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def build(name, command, environment=None):
    print(name, flush=True)
    receipt = {"name": name, "command": command, "started_utc": now()}
    with (ROOT / (name + ".log")).open("w") as log:
        result = subprocess.run(command, env=dict(os.environ, **(environment or {})),
                                stdout=log, stderr=subprocess.STDOUT)
    receipt.update(returncode=result.returncode, finished_utc=now())
    commands.append(receipt)
    save(ROOT / "qualification-commands.json", commands)
    if result.returncode:
        raise RuntimeError(f"{name} failed; see its log")


def measure(output, binary, args, receipt, metadata):
    receipt.update(command=[str(binary), *args], started_utc=now())
    result = subprocess.run(receipt["command"], text=True, capture_output=True)
    prefix = (f"group-{receipt['group']}-run-{receipt['run']}-{receipt['condition']}"
              if "group" in receipt else f"allocations-{receipt['condition']}")
    (output / (prefix + ".csv")).write_text(result.stdout)
    (output / (prefix + ".stderr")).write_text(result.stderr)
    receipt.update(returncode=result.returncode, finished_utc=now())
    metadata.setdefault("runs" if "group" in receipt else "allocation_runs", []).append(receipt)
    save(output / "environment.json", metadata)
    if result.returncode:
        raise RuntimeError(f"Measurement failed: {prefix}")
    print(prefix, flush=True)
    return result.stdout


state = json.loads((ROOT / "state.json").read_text())
checks = json.loads((ROOT / "checks.json").read_text())
assert len(checks) == 11 and all(c["returncode"] == 0 for c in checks)
assert json.loads((ROOT / "code-review-exit.json").read_text())["returncode"] == 0
assert json.loads((ROOT / "code-review-followup-exit.json").read_text())["returncode"] == 0
assert all(digest(REPO / p) == h for p, h in state["production_sha256"].items())
baseline = json.loads((ROOT / "baseline-provenance.json").read_text())
for workload in ["product", "headless"]:
    assert digest(ROOT / f"baseline-{workload}-bin") == baseline["binary_sha256"][workload]
commands = []

build("product-build-and-reference", [
    "python3", "scripts/compare_products.py", "--backend", "fork", "--profile", "production",
    "--samples", "24", "--warmup", "6", "--panes", "16,128,1024", "--runs", "4",
    "--out", str(ROOT / "candidate-product"),
])
product = json.loads((ROOT / "candidate-product/environment.json").read_text())
shutil.copy2(product["binary_paths"]["egui_dockspace"], ROOT / "candidate-product-bin")
assert digest(ROOT / "candidate-product-bin") == product["binary_sha256"]["egui_dockspace"]
assert product["benchmark_sources"] == baseline["benchmark_sources"]
assert product["imgui_sources"] == baseline["imgui_sources"]
changed = sorted(p for p in set(product["dockspace_sources"]) | set(baseline["sources"])
                 if product["dockspace_sources"].get(p) != baseline["sources"].get(p))
assert changed == sorted(state["production_scope"]), changed
orders = [(1, ["baseline", "candidate", "candidate", "baseline"]),
          (2, ["candidate", "baseline", "baseline", "candidate"])]
output = ROOT / "frozen-product"
output.mkdir(exist_ok=False)
metadata = {"baseline_commit": state["base"], "samples": 24, "warmup": 6,
            "panes": [16, 128, 1024], "source_changes": changed,
            "binary_sha256": {label: digest(ROOT / f"{label}-product-bin")
                              for label in ["baseline", "candidate"]}}
rows = []
for group, order in orders:
    for number, label in enumerate(order, 1):
        text = measure(output, ROOT / f"{label}-product-bin",
                       ["--samples", "24", "--warmup", "6", "--panes", "16,128,1024"],
                       dict(group=group, run=number, condition=label), metadata)
        rows.extend(dict(group=group, run=number, condition=label, **row)
                    for row in validate_rows(text, "egui_dockspace", [16, 128, 1024], 24))
write_csv(output / "raw.csv", list(rows[0]), rows)
summaries = []
for group, _ in orders:
    for label in ["baseline", "candidate"]:
        for panes in [16, 128, 1024]:
            for scenario in ["idle", "tab_click", "surface_resize"]:
                values = [r["elapsed_ns"] for r in rows
                          if (r["group"], r["condition"], r["panes"], r["scenario"])
                          == (group, label, panes, scenario)]
                summaries.append(dict(group=group, condition=label, panes=panes,
                                      scenario=scenario, samples=len(values),
                                      p50_ns=percentile(values, 50), p95_ns=percentile(values, 95),
                                      max_ns=max(values)))
write_csv(output / "summary.csv", list(summaries[0]), summaries)
metadata.update(finished_utc=now(), results_sha256={name: digest(output / name)
                for name in ["raw.csv", "summary.csv"]})
save(output / "environment.json", metadata)

environment = {"CARGO_BUILD_JOBS": "1", "CARGO_PROFILE_RELEASE_LTO": "thin",
               "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": "1"}
build("headless-build", ["cargo", "build", "--manifest-path",
      "integration/dockspace-host-harness/Cargo.toml", "--release", "--locked", "--offline",
      "--example", "frame_cost", "--message-format=json-render-diagnostics"], environment)
artifacts = set()
for line in (ROOT / "headless-build.log").read_text().splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    if (event.get("reason") == "compiler-artifact"
            and event.get("target", {}).get("name") == "frame_cost" and event.get("executable")):
        artifacts.add(Path(event["executable"]))
assert len(artifacts) == 1
shutil.copy2(artifacts.pop(), ROOT / "candidate-headless-bin")
output = ROOT / "headless"
output.mkdir(exist_ok=False)
metadata = {"baseline_commit": state["base"], "build_environment": environment,
            "scope": "Complete frame_cost workload; timing has inactive DHAT; process summaries only.",
            "binary_sha256": {label: digest(ROOT / f"{label}-headless-bin")
                              for label in ["baseline", "candidate"]}}
rows = []
for group, order in orders:
    for number, label in enumerate(order, 1):
        text = measure(output, ROOT / f"{label}-headless-bin", ["--timing-only", "--samples", "48",
                       "--warmup", "6", "--in-flight", "1", "--panes", "16,128,1024"],
                       dict(group=group, run=number, condition=label), metadata)
        samples = list(csv.DictReader(io.StringIO(text)))
        assert len(samples) == 30
        rows.extend(dict(group=group, run=number, condition=label, **row) for row in samples)
write_csv(output / "process-summaries.csv", list(rows[0]), rows)
for label in ["baseline", "candidate"]:
    measure(output, ROOT / f"{label}-headless-bin", ["--samples", "6", "--warmup", "2",
            "--in-flight", "1", "--panes", "16,128,1024"], dict(condition=label), metadata)
metadata.update(finished_utc=now(), results_sha256={name: digest(output / name)
                for name in ["process-summaries.csv", "allocations-baseline.csv", "allocations-candidate.csv"]})
save(output / "environment.json", metadata)
assert all(digest(REPO / p) == h for p, h in state["production_sha256"].items())
print("Qualification complete", flush=True)
