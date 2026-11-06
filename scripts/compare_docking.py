#!/usr/bin/env python3
"""Compare real renderer-free docking commands, all-root layout and owned state export."""

from __future__ import annotations

import argparse
from collections import defaultdict
import csv
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

from compare_products import (
    IMGUI_COMMIT, IMGUI_INPUTS, ROOT, capture, digest, percentile, utc_now, write_csv,
)


HEADER = [
    "product", "panes", "views", "case", "sample", "request_ns", "mutation_ns",
    "layout_ns", "export_ns", "operation_owned_ns", "nodes", "leaves", "splits",
    "items", "roots", "width", "height", "state_digest",
]
PHASES = ["request_ns", "mutation_ns", "layout_ns", "export_ns", "operation_owned_ns"]
CASES = ["resize", "center_move", "edge_split", "cross_view_move"]


def validate(text: str, product: str, panes: list[int], views: list[int],
             samples: int, warmup: int, selected_case: str | None) -> list[dict]:
    reader = csv.DictReader(io.StringIO(text))
    if reader.fieldnames != HEADER:
        raise RuntimeError(f"Unexpected docking columns from {product}: {reader.fieldnames}")
    cases = [selected_case] if selected_case else CASES
    expected = {(n, v, case, sample) for n in panes for v in views for case in cases
                if case != "cross_view_move" or v == 4 for sample in range(samples)}
    rows = list(reader)
    observed = set()
    for row in rows:
        for key in HEADER:
            if key not in ("product", "case", "state_digest"):
                row[key] = int(row[key])
        key = row["panes"], row["views"], row["case"], row["sample"]
        if row["product"] != product or key not in expected or key in observed:
            raise RuntimeError(f"Unexpected or duplicate docking sample: {row}")
        observed.add(key)
        n, v, case, sample = key
        extra = int(case == "edge_split")
        if ((row["nodes"], row["leaves"], row["splits"], row["items"], row["roots"])
                != (n * 2 - v + extra * 2, n + extra, n - v + extra, n * 2, v)):
            raise RuntimeError(f"Incomplete docking state: {row}")
        width = (16380, 16444)[(sample + warmup) % 2] if case == "resize" else 16380
        if row["width"] != width or row["height"] != 16380:
            raise RuntimeError(f"Wrong layout input: {row}")
        if (row["operation_owned_ns"] <= 0 or any(row[p] < 0 for p in PHASES)
                or sum(row[p] for p in PHASES[:-1]) != row["operation_owned_ns"]
                or len(row["state_digest"]) != 16
                or any(c not in "0123456789abcdef" for c in row["state_digest"])):
            raise RuntimeError(f"Invalid duration or state digest: {row}")
    if observed != expected:
        raise RuntimeError(f"Missing docking samples from {product}: {expected - observed}")
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=48)
    parser.add_argument("--warmup", type=int, default=8)
    parser.add_argument("--panes", default="16,128,1024")
    parser.add_argument("--views", default="1,4")
    parser.add_argument("--groups", type=int, choices=(1, 2), default=2)
    parser.add_argument("--case", choices=CASES)
    parser.add_argument("--imgui-root", type=Path, default=ROOT / "repo-ref/imgui")
    parser.add_argument("--cxx")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    panes = [int(value) for value in args.panes.split(",")]
    views = [int(value) for value in args.views.split(",")]
    if (not 1 <= args.samples <= 10_000 or not 0 <= args.warmup <= 10_000
            or not panes or len(set(panes)) != len(panes) or not set(panes) <= {16, 128, 1024}
            or not views or len(set(views)) != len(views) or not set(views) <= {1, 4}
            or args.case == "cross_view_move" and 4 not in views):
        parser.error("Use bounded samples/warmup, panes from 16,128,1024 and views from 1,4")
    reference = args.imgui_root.resolve()
    if capture(["git", "rev-parse", "HEAD"], cwd=reference) != IMGUI_COMMIT:
        parser.error(f"Expected pinned Dear ImGui {IMGUI_COMMIT}")
    if capture(["git", "diff", "--name-only", "HEAD", "--", *IMGUI_INPUTS], cwd=reference):
        parser.error("Pinned Dear ImGui inputs have local changes")
    compiler = args.cxx or shutil.which("clang++") or shutil.which("g++") or shutil.which("cl")
    if not compiler:
        parser.error("A C++17 compiler is required")
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=False)
    paths = capture([
        "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--",
        "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates/dockspace", "scripts",
        "integration/product-comparison",
    ]).split("\0")
    sources = {name: digest(ROOT / name) for name in paths
               if name and Path(name).suffix in (".rs", ".toml", ".lock", ".py", ".cpp")}
    environment = dict(os.environ, CARGO_BUILD_JOBS="1", DOCKSPACE_BENCH_NO_WORK_COUNTERS="1")
    metadata = {
        "started_utc": utc_now(), "source_commit": capture(["git", "rev-parse", "HEAD"]),
        "scope": "Actual docking request, all-root normalization/layout and fresh owned complete state; no renderer or host settlement.",
        "limits": "Logical views, not OS viewports. Command selection intent, not rendered visibility/click. Rust atomic validation and richer internal layout outputs remain included.",
        "setup_and_disposal": "Fresh fixture setup, validation, digest, IO, result/projection and context destruction are outside each total timer; real transaction temporary/replaced-state disposal remains inside apply.",
        "rust_build": "Production-profile in-crate ignored probe, with test structural-work ledger compiled out via DOCKSPACE_BENCH_NO_WORK_COUNTERS=1.",
        "build_environment": {k: environment[k] for k in ("CARGO_BUILD_JOBS", "DOCKSPACE_BENCH_NO_WORK_COUNTERS")},
        "samples": args.samples, "warmup": args.warmup, "panes": panes, "views": views,
        "selected_case": args.case, "groups": args.groups,
        "profile": "production, ThinLTO, one Rust codegen unit; C++ O3/NDEBUG with LTO",
        "imgui_commit": IMGUI_COMMIT, "rustc": capture(["rustc", "-Vv"]),
        "platform": platform.platform(), "sources": sources,
        "imgui_sources": {name: digest(reference / name) for name in IMGUI_INPUTS},
        "builds": [], "runs": [],
    }
    metadata_path = output / "environment.json"

    def save() -> None:
        metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")

    def build(name: str, command: list[str]) -> None:
        print("Building " + name, flush=True)
        receipt = {"name": name, "command": command, "started_utc": utc_now()}
        with (output / ("build-" + name + ".log")).open("w") as log:
            result = subprocess.run(command, cwd=ROOT, env=environment,
                                    stdout=log, stderr=subprocess.STDOUT)
        receipt.update(returncode=result.returncode, finished_utc=utc_now())
        metadata["builds"].append(receipt)
        save()
        if result.returncode:
            raise RuntimeError(f"Build failed: {name}; see its log")

    rust_command = ["cargo", "test", "--package", "dockspace", "--lib", "--profile", "production",
                    "--no-run", "--locked", "--offline", "--message-format=json-render-diagnostics"]
    save()
    build("rust", rust_command)
    executables = set()
    for line in (output / "build-rust.log").read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == "dockspace"
                and event.get("profile", {}).get("test") and event.get("executable")):
            executables.add(Path(event["executable"]))
            metadata["rust_artifact_profile"] = event["profile"]
    if len(executables) != 1:
        raise RuntimeError("Cargo did not identify one core unit-test executable")
    suffix = ".exe" if os.name == "nt" else ""
    binaries = {"dockspace": output / ("dockspace_docking_cost" + suffix),
                "dear_imgui": output / ("imgui_docking_cost" + suffix)}
    shutil.copy2(executables.pop(), binaries["dockspace"])
    cpp_sources = [str(ROOT / "integration/product-comparison/imgui_docking_cost.cpp")]
    cpp_sources += [str(reference / name) for name in IMGUI_INPUTS[1:4]]
    if Path(compiler).name.lower() in ("cl", "cl.exe"):
        objects = output / "objects"
        objects.mkdir()
        version = subprocess.run([compiler], text=True, capture_output=True)
        metadata["cxx"] = version.stdout + version.stderr
        cpp_command = [compiler, "/nologo", "/std:c++17", "/O2", "/DNDEBUG", "/GL", "/EHsc",
                       f"/I{reference}", *cpp_sources, f"/Fe:{binaries['dear_imgui']}",
                       f"/Fo:{objects}{os.sep}", "/link", "/LTCG"]
    else:
        metadata["cxx"] = capture([compiler, "--version"])
        lto = "-flto=thin" if "clang" in metadata["cxx"].lower() else "-flto"
        cpp_command = [compiler, "-std=c++17", "-O3", "-DNDEBUG", lto,
                       "-I", str(reference), *cpp_sources, "-o", str(binaries["dear_imgui"])]
    build("imgui", cpp_command)
    metadata["binary_sha256"] = {name: digest(path) for name, path in binaries.items()}
    rows = []
    for group in range(1, args.groups + 1):
        order = (("dockspace", "dear_imgui", "dear_imgui", "dockspace") if group == 1 else
                 ("dear_imgui", "dockspace", "dockspace", "dear_imgui"))
        for run, product in enumerate(order, 1):
            prefix = output / f"group-{group}-run-{run}-{product}"
            raw_path = prefix.with_suffix(".csv")
            state_dir = output / (prefix.name + "-states")
            state_dir.mkdir()
            command = [str(binaries[product])]
            if product == "dockspace":
                command += ["--exact", "docking_cost::docking_cost", "--ignored", "--nocapture", "--test-threads", "1"]
            measured_environment = dict(
                os.environ, DOCKSPACE_DOCK_COST_SAMPLES=str(args.samples),
                DOCKSPACE_DOCK_COST_WARMUP=str(args.warmup), DOCKSPACE_DOCK_COST_PANES=args.panes,
                DOCKSPACE_DOCK_COST_VIEWS=args.views, DOCKSPACE_DOCK_COST_OUT=str(raw_path),
                DOCKSPACE_DOCK_COST_STATE_DIR=str(state_dir),
            )
            if args.case:
                measured_environment["DOCKSPACE_DOCK_COST_CASE"] = args.case
            else:
                measured_environment.pop("DOCKSPACE_DOCK_COST_CASE", None)
            print(f"Group {group}, run {run}: {product}", flush=True)
            receipt = {"group": group, "run": run, "product": product,
                       "command": command, "started_utc": utc_now()}
            result = subprocess.run(command, cwd=ROOT, env=measured_environment,
                                    text=True, capture_output=True)
            prefix.with_suffix(".stdout").write_text(result.stdout)
            prefix.with_suffix(".stderr").write_text(result.stderr)
            if product == "dear_imgui":
                raw_path.write_text(result.stdout)
            receipt.update(returncode=result.returncode, finished_utc=utc_now())
            metadata["runs"].append(receipt)
            save()
            if result.returncode:
                raise RuntimeError(f"Docking probe failed; see {prefix}")
            rows.extend(dict(group=group, run=run, **row) for row in validate(
                raw_path.read_text(), product, panes, views, args.samples, args.warmup, args.case))
            expected_files = {
                f"{product}-{n}-{v}-{case}.txt" for n in panes for v in views
                for case in ([args.case] if args.case else CASES)
                if case != "cross_view_move" or v == 4
            }
            if {path.name for path in state_dir.iterdir()} != expected_files:
                raise RuntimeError(f"Missing or unexpected complete state dumps: {state_dir}")
    states = defaultdict(set)
    for row in rows:
        states[row["panes"], row["views"], row["case"], row["width"]].add(row["state_digest"])
    if any(len(values) != 1 for values in states.values()):
        write_csv(output / "raw-mismatch.csv", ["group", "run", *HEADER], rows)
        raise RuntimeError("Complete cross-product state mismatch; inspect normalized state files")
    dumps = defaultdict(set)
    for folder in output.glob("*-states"):
        for path in folder.glob("*.txt"):
            key = path.name.removeprefix("dockspace-").removeprefix("dear_imgui-")
            dumps[key].add(path.read_bytes())
    # Resize has two widths, but one first-sample export per process.
    expected_dumps = {(r["panes"], r["views"], r["case"]) for r in rows}
    if len(dumps) != len(expected_dumps):
        raise RuntimeError("Missing complete first-sample state dumps")
    if any(len(values) != 1 for values in dumps.values()):
        raise RuntimeError("Complete normalized first-sample vectors differ")
    summaries = []
    for group in range(1, args.groups + 1):
        for product in binaries:
            for count in panes:
                for view_count in views:
                    for case in ([args.case] if args.case else CASES):
                        subset = [r for r in rows if (r["group"], r["product"], r["panes"], r["views"], r["case"])
                                  == (group, product, count, view_count, case)]
                        if not subset:
                            continue
                        summary = dict(group=group, product=product, panes=count, views=view_count,
                                       case=case, samples=len(subset))
                        for phase in PHASES:
                            values = [r[phase] for r in subset]
                            summary.update({phase + "_p50": percentile(values, 50),
                                            phase + "_p95": percentile(values, 95), phase + "_max": max(values)})
                        summaries.append(summary)
    write_csv(output / "raw.csv", ["group", "run", *HEADER], rows)
    write_csv(output / "summary.csv", list(summaries[0]), summaries)
    metadata.update(finished_utc=utc_now(), complete_state_dumps_equal=True,
                    results_sha256={name: digest(output / name) for name in ("raw.csv", "summary.csv")})
    if any(digest(ROOT / name) != value for name, value in sources.items()):
        raise RuntimeError("Source inputs changed during build or measurement")
    if any(digest(reference / name) != value for name, value in metadata["imgui_sources"].items()):
        raise RuntimeError("Pinned reference inputs changed during measurement")
    if any(digest(path) != metadata["binary_sha256"][name] for name, path in binaries.items()):
        raise RuntimeError("A docking executable changed during measurement")
    metadata["input_hashes_unchanged_after_run"] = True
    save()
    print(f"Complete renderer-free docking comparison: {output}")


if __name__ == "__main__":
    main()
