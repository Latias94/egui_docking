"""Sum disjoint matching roots in one macOS sample main-thread call tree."""
import hashlib, json, re, sys
from pathlib import Path
source = Path(sys.argv[1])
graph = source.read_text().split('Call graph:', 1)[1].split('Total number in stack', 1)[0]
total = int(re.search(r'(?m)^\s+(\d+) Thread_.*com.apple.main-thread', graph)[1])
patterns = {
    'paint_audit': ['output_proof::', 'output_proof..'],
    'layout_solver': ['dockspace::layout::'],
    'scene_compiler': ['dockspace::scene_compiler::'],
    'scene_validation': ['dockspace::scene::validation::'],
    'measurement_and_resources': ['egui_dockspace::product_render::measurement::'],
    'junction_queries': ['splitter_junction_axis_operable'],
    'cpu_tessellation': ['egui::context::Context::tessellate'],
    'product_paint': ['egui_dockspace::product_render::paint_surface::'],
}
summary = {}
for category, needles in patterns.items():
    stack, roots = [], []
    for line in graph.splitlines():
        match = re.match(r'^([ +!:\\|]*)\s?(\d+) (.+)', line)
        if not match:
            continue
        depth = match.start(2)
        while stack and stack[-1][0] >= depth:
            stack.pop()
        matched = any(needle in match[3] for needle in needles)
        if matched and not any(item[1] for item in stack):
            roots.append({'samples': int(match[2]), 'node': match[3].split('  (in', 1)[0]})
        stack.append((depth, matched))
    count = sum(item['samples'] for item in roots)
    if count > total:
        raise RuntimeError('Overlapping root count')
    summary[category] = {'inclusive_union_samples': count, 'percent': round(count / total * 100, 3), 'nonoverlapping_roots': roots}
print(json.dumps({'source': str(source), 'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'total_thread_samples': total, 'scope': 'Each category sums disjoint roots; categories can overlap each other. Inlined work may be charged to callers. Sample counts are not per-frame wall times.', 'categories': summary}, indent=2))
