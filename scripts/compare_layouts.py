#!/usr/bin/env python3
"""Compare renderer-free layout projection, retaining each implementation's costs."""

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
    IMGUI_COMMIT, IMGUI_INPUTS, ROOT, build, capture, digest, percentile, utc_now, write_csv,
)

HEADER = [
    "product", "panes", "case", "sample", "preparation_ns", "elapsed_ns",
    "nodes", "leaves", "splits", "width", "height", "geometry_digest",
]
CASES = {"dockspace": ("project_full", "project_prepared"), "dear_imgui": ("project_in_place",)}


def validate(text: str, product: str, panes: list[int], samples: int, warmup: int) -> list[dict]:
    reader = csv.DictReader(io.StringIO(text))
    if reader.fieldnames != HEADER:
        raise RuntimeError(f"Unexpected columns from {product}")
    rows = list(reader)
    expected = {(n, case, sample) for n in panes for case in CASES[product] for sample in range(samples)}
    observed = set()
    for row in rows:
        for key in HEADER:
            if key not in ("product", "case", "geometry_digest"):
                row[key] = int(row[key])
        key = row["panes"], row["case"], row["sample"]
        count = row["panes"]
        if row["product"] != product or key not in expected or key in observed:
            raise RuntimeError(f"Unexpected or duplicate sample: {row}")
        observed.add(key)
        if (row["nodes"], row["leaves"], row["splits"]) != (count * 2 - 1, count, count - 1):
            raise RuntimeError(f"Incomplete geometry: {row}")
        if row["width"] != (16380, 16444)[(row["sample"] + warmup) % 2] or row["height"] != 16380:
            raise RuntimeError(f"Resize sequence mismatch: {row}")
        if row["elapsed_ns"] <= 0 or row["preparation_ns"] < 0:
            raise RuntimeError(f"Invalid duration: {row}")
    if observed != expected:
        raise RuntimeError(f"Missing samples from {product}")
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=96)
    parser.add_argument("--warmup", type=int, default=16)
    parser.add_argument("--panes", default="16,128,1024")
    parser.add_argument("--groups", type=int, choices=(1, 2), default=2)
    parser.add_argument("--imgui-root", type=Path, default=ROOT / "repo-ref/imgui")
    parser.add_argument("--cxx")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    panes = [int(value) for value in args.panes.split(",")]
    if (not 1 <= args.samples <= 1_000_000 or not 0 <= args.warmup <= 1_000_000
            or not panes or len(set(panes)) != len(panes) or not set(panes) <= {16, 128, 1024}):
        parser.error("Use bounded positive samples, nonnegative warmup and unique panes from 16,128,1024")
    reference = args.imgui_root.resolve()
    if capture(["git", "rev-parse", "HEAD"], cwd=reference) != IMGUI_COMMIT:
        parser.error(f"Expected pinned Dear ImGui {IMGUI_COMMIT}")
    if capture(["git", "diff", "--name-only", "HEAD", "--", *IMGUI_INPUTS], cwd=reference):
        parser.error("Measured Dear ImGui inputs have local changes")
    compiler = args.cxx or shutil.which("clang++") or shutil.which("g++") or shutil.which("cl")
    if not compiler:
        parser.error("A C++17 compiler is required")
    output = args.out.resolve()
    output.mkdir(parents=True, exist_ok=False)
    rust_command = [
        "cargo", "test", "--package", "dockspace", "--lib", "--profile", "production",
        "--no-run", "--locked", "--offline", "--message-format=json-render-diagnostics",
    ]
    paths = capture(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--",
                     "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates/dockspace", "scripts",
                     "integration/product-comparison"]).split("\0")
    sources = {name: digest(ROOT / name) for name in paths
               if name and Path(name).suffix in (".rs", ".toml", ".lock", ".py", ".cpp")}
    metadata = {
        "started_utc": utc_now(), "source_commit": capture(["git", "rev-parse", "HEAD"]),
        "scope": "Renderer-free balanced binary layout; no NewFrame, Begin/End, text or tessellation.",
        "limits": "Rust returns a richer immutable projection; ImGui mutates existing node bounds. Not full docking feature parity or a product speed ratio.",
        "rust_test_build": "Ignored in-crate probe for private production functions; production profile, debug assertions off, no layout work counters.",
        "prepared_case": "Root measurement occurs outside the projection timer and is reported separately as preparation_ns. project_full includes it.",
        "disposal": "Outside each timer; in-place ImGui has no fresh projection to release.",
        "geometry": "All nodes checked against exact common-subset geometry after each operation; output digest compared across products and cases.",
        "samples": args.samples, "warmup": args.warmup, "panes": panes,
        "groups": args.groups, "profile": "production, ThinLTO, one Rust codegen unit",
        "imgui_commit": IMGUI_COMMIT, "rustc": capture(["rustc", "-Vv"]),
        "platform": platform.platform(), "build_commands": [rust_command], "runs": [],
        "sources": sources,
        "imgui_sources": {name: digest(reference / name) for name in IMGUI_INPUTS},
    }
    metadata_path = output / "environment.json"
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print("Building private Rust layout probe with one Cargo job...", flush=True)
    metadata["rust_build_started_utc"] = utc_now()
    build(rust_command, output / "build-rust.log")
    metadata["rust_build_finished_utc"] = utc_now()
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
        raise RuntimeError("Cargo did not identify one unit-test executable")
    binary = output / ("imgui_layout_cost.exe" if os.name == "nt" else "imgui_layout_cost")
    cpp_sources = [str(ROOT / "integration/product-comparison/imgui_layout_cost.cpp")]
    cpp_sources += [str(reference / name) for name in IMGUI_INPUTS[1:4]]
    if Path(compiler).name.lower() in ("cl", "cl.exe"):
        objects = output / "objects"
        objects.mkdir()
        version = subprocess.run([compiler], text=True, capture_output=True)
        compiler_version = version.stdout + version.stderr
        cpp_command = [compiler, "/nologo", "/std:c++17", "/O2", "/DNDEBUG", "/GL", "/EHsc",
                       f"/I{reference}", *cpp_sources, f"/Fe:{binary}", f"/Fo:{objects}{os.sep}", "/link", "/LTCG"]
    else:
        compiler_version = capture([compiler, "--version"])
        lto = "-flto=thin" if "clang" in compiler_version.lower() else "-flto"
        cpp_command = [compiler, "-std=c++17", "-O3", "-DNDEBUG", lto, "-I", str(reference),
                       *cpp_sources, "-o", str(binary)]
    metadata["cxx"] = compiler_version
    metadata["build_commands"].append(cpp_command)
    metadata["cpp_build_started_utc"] = utc_now()
    print("Building pinned ImGui layout routine...", flush=True)
    build(cpp_command, output / "build-imgui.log")
    metadata["cpp_build_finished_utc"] = utc_now()
    binaries = {"dockspace": executables.pop(), "dear_imgui": binary}
    metadata["binary_sha256"] = {name: digest(path) for name, path in binaries.items()}
    rows = []
    environment = dict(os.environ, DOCKSPACE_LAYOUT_COST_SAMPLES=str(args.samples),
                       DOCKSPACE_LAYOUT_COST_WARMUP=str(args.warmup), DOCKSPACE_LAYOUT_COST_PANES=args.panes)
    for group in range(1, args.groups + 1):
        for run, product in enumerate(("dockspace", "dear_imgui", "dear_imgui", "dockspace"), 1):
            prefix = output / f"group-{group}-run-{run}-{product}"
            raw_path = prefix.with_suffix(".csv")
            command = [str(binaries[product])]
            if product == "dockspace":
                command += ["--exact", "layout::layout_cost::layout_cost", "--ignored", "--nocapture", "--test-threads", "1"]
            print(f"Group {group}, run {run}: {product}", flush=True)
            started = utc_now()
            result = subprocess.run(command, cwd=ROOT, env=dict(environment, DOCKSPACE_LAYOUT_COST_OUT=str(raw_path)),
                                    text=True, capture_output=True)
            prefix.with_suffix(".stdout").write_text(result.stdout)
            prefix.with_suffix(".stderr").write_text(result.stderr)
            if product == "dear_imgui":
                raw_path.write_text(result.stdout)
            metadata["runs"].append({"group": group, "run": run, "product": product, "command": command,
                                     "started_utc": started, "finished_utc": utc_now(), "returncode": result.returncode})
            metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
            if result.returncode:
                raise RuntimeError(f"Layout probe failed; see {prefix}")
            rows.extend(dict(group=group, run=run, **row) for row in
                        validate(raw_path.read_text(), product, panes, args.samples, args.warmup))
    geometries = defaultdict(set)
    for row in rows:
        geometries[row["panes"], row["width"]].add(row["geometry_digest"])
    if any(len(values) != 1 for values in geometries.values()):
        raise RuntimeError("Cross-product geometry mismatch")
    summaries = []
    for group in range(1, args.groups + 1):
        for product, cases in CASES.items():
            for count in panes:
                for case in cases:
                    subset = [r for r in rows if (r["group"], r["product"], r["panes"], r["case"]) == (group, product, count, case)]
                    values = [r["elapsed_ns"] for r in subset]
                    preparations = [r["preparation_ns"] for r in subset]
                    summaries.append(dict(group=group, product=product, panes=count, case=case, samples=len(values),
                                          p50_ns=percentile(values, 50), p95_ns=percentile(values, 95), max_ns=max(values),
                                          preparation_p50_ns=percentile(preparations, 50)))
    write_csv(output / "raw.csv", ["group", "run", *HEADER], rows)
    write_csv(output / "summary.csv", list(summaries[0]), summaries)
    metadata["finished_utc"] = utc_now()
    metadata["results_sha256"] = {name: digest(output / name) for name in ("raw.csv", "summary.csv")}
    if any(digest(ROOT / name) != value for name, value in sources.items()):
        raise RuntimeError("Source inputs changed during build or measurement")
    if any(digest(reference / name) != value for name, value in metadata["imgui_sources"].items()):
        raise RuntimeError("Pinned ImGui inputs changed during measurement")
    if any(digest(path) != metadata["binary_sha256"][name] for name, path in binaries.items()):
        raise RuntimeError("An executable changed during measurement")
    metadata["input_hashes_unchanged_after_run"] = True
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Complete layout samples and verified geometry: {output}")


if __name__ == "__main__":
    main()
