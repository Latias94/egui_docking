#!/usr/bin/env python3
"""Build and compare complete CPU draw-data generation for two docking products."""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
from datetime import datetime, timezone


ROOT = Path(__file__).resolve().parents[1]
IMGUI_COMMIT = "d88f238d1d363ef387382b2e29ad12cf96f826ac"
MANIFEST = ROOT / "integration/egui-product-harness/Cargo.toml"
TARGET = MANIFEST.parent / "target/release"
HEADER = [
    "product", "panes", "tabs_per_pane", "scenario", "sample", "frames",
    "elapsed_ns", "passes", "content_calls", "visible_contents", "vertices",
    "indices", "draw_commands",
]
SCENARIOS = {"idle": 1, "tab_click": 2, "surface_resize": 1}
PRODUCTS = ("egui_dockspace", "dear_imgui")
IMGUI_INPUTS = [
    "imgui.cpp", "imgui_draw.cpp", "imgui_tables.cpp", "imgui_widgets.cpp",
    "imgui.h", "imgui_internal.h", "imconfig.h", "imstb_rectpack.h",
    "imstb_textedit.h", "imstb_truetype.h",
]


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(command: list[str], *, cwd: Path = ROOT) -> str:
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(f"Command failed: {command!r}\n{result.stderr[-6000:]}")
    return result.stdout.strip()


def build(command: list[str], log: Path) -> None:
    environment = dict(os.environ, CARGO_BUILD_JOBS="1")
    with log.open("w", encoding="utf-8") as stream:
        result = subprocess.run(
            command, cwd=ROOT, env=environment,
            stdout=stream, stderr=subprocess.STDOUT,
        )
    if result.returncode:
        raise RuntimeError(f"Build failed; see {log}")


def validate_rows(text: str, product: str, panes: list[int], samples: int) -> list[dict]:
    reader = csv.DictReader(io.StringIO(text))
    if reader.fieldnames != HEADER:
        raise RuntimeError(f"Unexpected CSV header from {product}: {reader.fieldnames}")
    rows = list(reader)
    expected = {(n, scenario, sample)
                for n in panes for scenario in SCENARIOS for sample in range(samples)}
    observed = set()
    for row in rows:
        if row["product"] != product:
            raise RuntimeError(f"Wrong product label: {row!r}")
        for key in HEADER[1:]:
            if key != "scenario":
                row[key] = int(row[key])
        key = row["panes"], row["scenario"], row["sample"]
        if key not in expected or key in observed:
            raise RuntimeError(f"Unexpected or repeated sample: {key}")
        observed.add(key)
        if row["tabs_per_pane"] != 2 or row["frames"] != SCENARIOS[row["scenario"]]:
            raise RuntimeError(f"Mismatched workload: {row!r}")
        if row["passes"] < row["frames"]:
            raise RuntimeError(f"A logical frame was not rendered: {row!r}")
        if row["visible_contents"] != row["panes"] * row["passes"]:
            raise RuntimeError(f"Missing visible pane content: {row!r}")
        if row["content_calls"] < row["visible_contents"]:
            raise RuntimeError(f"Visible contents were not drawn: {row!r}")
        if any(row[key] <= 0 for key in ("elapsed_ns", "vertices", "indices", "draw_commands")):
            raise RuntimeError(f"Empty timing or draw data: {row!r}")
    if observed != expected:
        raise RuntimeError(f"Missing samples from {product}: {sorted(expected - observed)}")
    return rows


def percentile(values: list[int], percent: int) -> int:
    return sorted(values)[math.ceil(len(values) * percent / 100) - 1]


def write_csv(path: Path, header: list[str], rows: list[dict]) -> None:
    with path.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=header, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=24)
    parser.add_argument("--warmup", type=int, default=6)
    parser.add_argument("--panes", default="16,128,1024")
    parser.add_argument("--runs", type=int, choices=(2, 4), default=4)
    parser.add_argument("--imgui-root", type=Path, default=ROOT / "repo-ref/imgui")
    parser.add_argument("--cxx", help="C++ compiler executable (clang++, g++, or cl)")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    panes = [int(value) for value in args.panes.split(",")]
    if (args.samples <= 0 or args.warmup < 0 or not panes
            or len(set(panes)) != len(panes) or not set(panes) <= {16, 128, 1024}):
        parser.error("Use positive samples, nonnegative warmup, and unique panes from 16,128,1024")
    reference = args.imgui_root.resolve()
    actual_commit = capture(["git", "rev-parse", "HEAD"], cwd=reference)
    if actual_commit != IMGUI_COMMIT:
        parser.error(f"Expected Dear ImGui {IMGUI_COMMIT}, found {actual_commit}; use --imgui-root")
    dirty = capture(["git", "diff", "--name-only", "HEAD", "--", *IMGUI_INPUTS], cwd=reference)
    if dirty:
        parser.error(f"Measured Dear ImGui sources have local changes: {dirty}")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = (args.out or MANIFEST.parent / "target/product-comparison" / stamp).resolve()
    output.mkdir(parents=True, exist_ok=False)
    executable_suffix = ".exe" if os.name == "nt" else ""
    binaries = {
        "egui_dockspace": TARGET / "examples" / f"product_cost{executable_suffix}",
        "dear_imgui": TARGET / f"imgui_product_cost{executable_suffix}",
    }
    compiler = args.cxx or shutil.which("clang++") or shutil.which("g++") or shutil.which("cl")
    if not compiler:
        parser.error("Install a C++17 compiler or provide --cxx")
    msvc = Path(compiler).name.lower() in {"cl", "cl.exe"}
    sources = [str(ROOT / "integration/product-comparison/imgui_product_cost.cpp")]
    sources.extend(str(reference / name) for name in IMGUI_INPUTS[:4])
    if msvc:
        objects = output / "objects"
        objects.mkdir()
        cpp_command = [compiler, "/nologo", "/std:c++17", "/O2", "/DNDEBUG", "/EHsc",
                       f"/I{reference}", *sources, f"/Fe:{binaries['dear_imgui']}",
                       f"/Fo:{objects}{os.sep}"]
        version = subprocess.run([compiler], text=True, capture_output=True)
        compiler_version = (version.stdout + version.stderr).strip()
    else:
        cpp_command = [compiler, "-std=c++17", "-O3", "-DNDEBUG", "-I", str(reference),
                       *sources, "-o", str(binaries["dear_imgui"])]
        compiler_version = capture([compiler, "--version"])
    rust_command = ["cargo", "build", "--manifest-path", str(MANIFEST), "--release",
                    "--locked", "--offline", "--example", "product_cost",
                    "--message-format=json-render-diagnostics"]
    metadata = {
        "started_utc": utc_now(), "source_commit": capture(["git", "rev-parse", "HEAD"]),
        "imgui_commit": actual_commit, "imgui_root": str(reference),
        "rustc": capture(["rustc", "-Vv"]), "cxx": compiler_version,
        "platform": platform.platform(), "machine": platform.machine(),
        "build_commands": [rust_command, cpp_command],
        "allocation": "Normal product System allocation; no heap instrumentation or RSS ratio.",
        "scope": "CPU complete draw-data generation, no GPU/native window/vsync; full content and meshes.",
        "samples": args.samples, "warmup": args.warmup, "panes": panes,
        "runs": [], "benchmark_sources": {},
        "imgui_sources": {name: digest(reference / name) for name in IMGUI_INPUTS},
    }
    for name in ["scripts/compare_products.py", "integration/product-comparison/imgui_product_cost.cpp",
                 "integration/egui-product-harness/examples/product_cost.rs"]:
        metadata["benchmark_sources"][name] = digest(ROOT / name)
    source_paths = capture([
        "git", "ls-files", "-z", "--", "Cargo.toml", "Cargo.lock",
        "rust-toolchain.toml", "crates", "integration",
    ]).split("\0")
    metadata["dockspace_sources"] = {
        name: digest(ROOT / name) for name in source_paths
        if name and Path(name).suffix in {".rs", ".toml", ".lock"}
    }
    metadata_path = output / "environment.json"
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    TARGET.mkdir(parents=True, exist_ok=True)
    print("Building egui product (one Cargo job)...", flush=True)
    build(rust_command, output / "build-egui.log")
    rust_artifacts = set()
    for line in (output / "build-egui.log").read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (event.get("reason") == "compiler-artifact"
                and event.get("target", {}).get("name") == "product_cost"
                and "example" in event["target"].get("kind", [])
                and event.get("executable")):
            rust_artifacts.add(Path(event["executable"]).resolve())
    if len(rust_artifacts) != 1:
        raise RuntimeError("Cargo did not identify exactly one product_cost executable")
    binaries["egui_dockspace"] = rust_artifacts.pop()
    metadata["binary_paths"] = {name: str(path) for name, path in binaries.items()}
    print("Building pinned Dear ImGui product...", flush=True)
    build(cpp_command, output / "build-imgui.log")
    for product, binary in binaries.items():
        if not binary.is_file():
            raise RuntimeError(f"Missing {product} executable: {binary}")
    metadata["binary_sha256"] = {name: digest(path) for name, path in binaries.items()}
    rows = []
    order = [PRODUCTS[0], PRODUCTS[1], PRODUCTS[1], PRODUCTS[0]][:args.runs]
    for run, product in enumerate(order, 1):
        command = [str(binaries[product]), "--samples", str(args.samples),
                   "--warmup", str(args.warmup), "--panes", args.panes]
        print(f"Run {run}/{args.runs}: {product}", flush=True)
        started = utc_now()
        result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
        (output / f"run-{run}-{product}.csv").write_text(result.stdout)
        (output / f"run-{run}-{product}.stderr").write_text(result.stderr)
        metadata["runs"].append({"run": run, "product": product, "command": command,
                                 "started_utc": started, "finished_utc": utc_now(),
                                 "returncode": result.returncode})
        metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
        if result.returncode:
            raise RuntimeError(f"{product} failed: {result.stderr[-6000:]}")
        rows.extend({"run": run, **row} for row in validate_rows(result.stdout, product, panes, args.samples))
    write_csv(output / "raw.csv", ["run", *HEADER], rows)
    summaries = []
    for product in PRODUCTS:
        for n in panes:
            for scenario, frames in SCENARIOS.items():
                selected = [row for row in rows if row["product"] == product
                            and row["panes"] == n and row["scenario"] == scenario]
                values = [row["elapsed_ns"] for row in selected]
                summary = {"product": product, "panes": n, "scenario": scenario,
                           "frames_per_sample": frames, "samples": len(selected),
                           "p50_ns": percentile(values, 50), "p95_ns": percentile(values, 95),
                           "max_ns": max(values)}
                for key in ["passes", "content_calls", "visible_contents", "vertices", "indices", "draw_commands"]:
                    summary[f"p50_{key}"] = percentile([row[key] for row in selected], 50)
                summaries.append(summary)
    write_csv(output / "summary.csv", list(summaries[0]), summaries)
    metadata["finished_utc"] = utc_now()
    metadata["results_sha256"] = {name: digest(output / name) for name in ["raw.csv", "summary.csv"]}
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Wrote complete raw samples, summaries, build logs and provenance to {output}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Product comparison failed: {error}", file=sys.stderr)
        raise SystemExit(1)
