"""Check archive hashes, product percentiles, coverage and equal rendered work."""

from collections import Counter
import csv
import hashlib
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def read_csv(path):
    return list(csv.DictReader((ROOT / path).open()))


def read_json(path):
    return json.loads((ROOT / path).read_text())


def percentile(values, percent):
    return sorted(values)[math.ceil(len(values) * percent / 100) - 1]


def verify_product(directory, grouped):
    rows = read_csv(directory + "/raw.csv")
    metadata = read_json(directory + "/environment.json")
    labels = ([(g, label) for g in [1, 2] for label in ["baseline", "candidate"]]
              if grouped else [(label,) for label in ["egui_dockspace", "dear_imgui"]])
    metrics = ["passes", "content_calls", "visible_contents", "vertices", "indices", "draw_commands"]
    for label in labels:
        selected = [r for r in rows if ((int(r["group"]), r["condition"]) if grouped
                                       else (r["product"],)) == label]
        for panes in [16, 128, 1024]:
            for scenario, frames in [("idle", 1), ("tab_click", 2), ("surface_resize", 1)]:
                samples = [r for r in selected if (int(r["panes"]), r["scenario"]) == (panes, scenario)]
                assert len(samples) == 48
                assert Counter(int(r["sample"]) for r in samples) == Counter({n: 2 for n in range(24)})
                for r in samples:
                    assert int(r["tabs_per_pane"]) == 2 and int(r["frames"]) == frames
                    assert int(r["passes"]) >= frames
                    assert int(r["visible_contents"]) == panes * int(r["passes"])
                    assert int(r["content_calls"]) >= int(r["visible_contents"])
                    assert all(int(r[k]) > 0 for k in ["elapsed_ns", "vertices", "indices", "draw_commands"])
                summary = [r for r in read_csv(directory + "/summary.csv")
                           if ((int(r["group"]), r["condition"]) if grouped else (r["product"],)) == label
                           and int(r["panes"]) == panes and r["scenario"] == scenario]
                assert len(summary) == 1
                values = [int(r["elapsed_ns"]) for r in samples]
                assert int(summary[0]["samples"]) == len(values)
                assert int(summary[0]["p50_ns"]) == percentile(values, 50)
                assert int(summary[0]["p95_ns"]) == percentile(values, 95)
                assert int(summary[0]["max_ns"]) == max(values)
    if grouped:
        for group in [1, 2]:
            work = {label: Counter(tuple(r[k] for k in ["panes", "scenario", "sample", "frames", *metrics])
                                  for r in rows if int(r["group"]) == group and r["condition"] == label)
                    for label in ["baseline", "candidate"]}
            assert work["baseline"] == work["candidate"]
    for name, expected in metadata["results_sha256"].items():
        assert hashlib.sha256((ROOT / directory / name).read_bytes()).hexdigest() == expected
    return len(rows)


manifest = ROOT / "SHA256SUMS"
if manifest.exists():
    for line in manifest.read_text().splitlines():
        expected, name = line.split("  ", 1)
        assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, name
checks = read_json("checks.json")
assert len(checks) == 11 and all(c["returncode"] == 0 for c in checks)
inventory = read_json("state.json")["production_sha256"]
expected_source = hashlib.sha256(json.dumps(inventory, sort_keys=True).encode()).hexdigest()
assert all(c["source_snapshot_sha256"] == expected_source for c in checks)
assert read_json("code-review-followup-exit.json")["returncode"] == 0
products = verify_product("candidate-product", False)
frozen = verify_product("frozen-product", True)
expected_cases = {(str(panes), scenario, phase) for panes in [16, 128, 1024]
                  for scenario in ["idle", "select", "drag", "measurement_change", "outputs_in_flight"]
                  for phase in ["update", "settlement"]}
headless = read_csv("headless/process-summaries.csv")
for group, order in [(1, ["baseline", "candidate", "candidate", "baseline"]),
                     (2, ["candidate", "baseline", "baseline", "candidate"])]:
    for run, label in enumerate(order, 1):
        rows = [r for r in headless if (int(r["group"]), int(r["run"]), r["condition"]) == (group, run, label)]
        assert len(rows) == 30
        assert {(r["panes"], r["scenario"], r["phase"]) for r in rows} == expected_cases
        for r in rows:
            assert r["metric"] == "time_ns" and r["instrumentation"] == "dhat_inactive"
            assert r["samples"] == "48" and r["warmup"] == "6" and r["tabs_per_pane"] == "2"
            assert 0 < int(r["p50"]) <= int(r["p95"]) <= int(r["max"])
            outputs = (1, 2) if r["scenario"] == "outputs_in_flight" else (0, 1)
            expected = outputs if r["phase"] == "update" else (outputs[1], 0)
            assert (int(r["outputs_before"]), int(r["outputs_after"])) == expected
            assert r["end_state"] == ("retained_in_flight" if r["phase"] == "update" else "settled")
allocation_rows = 0
for label in ["baseline", "candidate"]:
    rows = read_csv(f"headless/allocations-{label}.csv")
    assert len(rows) == 180
    for metric in ["time_ns", "allocation_events", "requested_bytes", "live_before_tracked_bytes",
                   "live_after_tracked_bytes", "case_prefix_peak_tracked_bytes"]:
        selected = [r for r in rows if r["metric"] == metric]
        assert len(selected) == 30
        assert {(r["panes"], r["scenario"], r["phase"]) for r in selected} == expected_cases
        for r in selected:
            assert r["samples"] == "6" and r["warmup"] == "2"
            assert r["instrumentation"] == ("dhat_inactive" if metric == "time_ns" else "dhat_active")
            assert 0 <= int(r["p50"]) <= int(r["p95"]) <= int(r["max"])
    allocation_rows += len(rows)
metadata = read_json("headless/environment.json")
for name, expected in metadata["results_sha256"].items():
    assert hashlib.sha256((ROOT / "headless" / name).read_bytes()).hexdigest() == expected
print(json.dumps({"product_raw_rows": products, "frozen_product_raw_rows": frozen,
                  "headless_process_summaries": len(headless), "headless_allocation_rows": allocation_rows,
                  "product_percentiles_recomputed": True, "rendered_work_equal": True,
                  "headless_sample_percentiles_available": False, "checks_passed": len(checks)}, indent=2))
