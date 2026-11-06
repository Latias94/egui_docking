"""Reproduce inclusive sample categories; categories overlap, not frame budgets."""

from pathlib import Path
import gzip
import hashlib
import json
import re


PATTERNS = {
    "paint_output_audit": ["output_proof::", "output_proof.."],
    "numerical_layout": ["dockspace::layout::"],
    "scene_compiler": ["dockspace::scene_compiler::"],
    "scene_validation": ["dockspace::scene::validation::"],
    "hit_and_touch": ["dockspace::presentation_hit::", "dockspace..presentation_hit.."],
    "semantic_manifest": ["dockspace::semantic_manifest::"],
    "measurement_resources": ["egui_dockspace::product_render::measurement::"],
    "product_paint": ["egui_dockspace::product_render::paint_surface::"],
    "receiver_queries": ["dockspace::runtime::paint::SurfacePaintPlan::receiver::"],
    "junction_queries": ["splitter_junction_axis_operable"],
    "egui_widget_creation": ["egui::context::Context::create_widget::"],
    "egui_response_queries": ["egui::context::Context::get_response::"],
    "egui_child_ui": ["egui::ui::Ui::new_child::"],
    "cpu_tessellation": ["egui::context::Context::tessellate"],
    "immutable_hit_value_eq": ["HitGeometry$u20$as$u20$core..cmp..PartialEq"],
    "immutable_scene_layout_eq": ["PresentationLayoutFacts$u20$as$u20$core..cmp..PartialEq"],
    "scene_value_comparison": ["SurfaceSceneSet$u20$as$u20$core..cmp..PartialEq"],
    "measurement_map_insertion": ["dockspace::scene_manifest::SurfaceMeasurements::insert_"],
    "requirement_refresh": ["try_rebuild_presentation_requirements::"],
}


def summarize(raw):
    graph = raw.decode().split("Call graph:", 1)[1].split("Total number in stack", 1)[0]
    # Both archived reports contain one thread; do not mix another thread into its denominator.
    threads = re.findall(r"(?m)^\s+(\d+) Thread_.*", graph)
    assert len(threads) == 1 and "com.apple.main-thread" in graph
    total = int(threads[0])
    categories = {}
    for name, needles in PATTERNS.items():
        stack, roots = [], []
        for line in graph.splitlines():
            match = re.match(r"^([ +!:\|]*)\s?(\d+) (.+)", line)
            if not match:
                continue
            depth = match.start(2)
            while stack and stack[-1][0] >= depth:
                stack.pop()
            matched = any(needle in match[3] for needle in needles)
            if matched and not any(entry[1] for entry in stack):
                roots.append({"samples": int(match[2]), "node": match[3].split("  (in", 1)[0]})
            stack.append((depth, matched))
        count = sum(entry["samples"] for entry in roots)
        assert count <= total
        categories[name] = {
            "samples": count,
            "percent": round(100 * count / total, 3),
            "roots": roots,
        }
    return total, categories


def main():
    directory = Path(__file__).resolve().parent
    for case in ("idle", "tab_click"):
        raw = gzip.decompress((directory / f"current-{case}.sample.txt.gz").read_bytes())
        recorded = json.loads((directory / f"current-{case}-attribution.json").read_text())
        total, categories = summarize(raw)
        assert hashlib.sha256(raw).hexdigest() == recorded["source_sha256"]
        assert total == recorded["total_main_thread_samples"]
        assert categories == recorded["categories"]
        print(case, total, {name: entry["percent"] for name, entry in categories.items()})
    print("Raw hashes and inclusive categories match; categories overlap and are not savings.")


if __name__ == "__main__":
    main()
