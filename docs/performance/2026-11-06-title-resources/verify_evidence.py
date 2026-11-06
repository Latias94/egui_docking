"""Recompute CPU percentiles, allocation counts, source scope and timing separation."""
from collections import Counter
from datetime import datetime
import csv
import hashlib
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent

def load(name):
    return json.loads((ROOT / name).read_text())

def rows(name):
    return list(csv.DictReader((ROOT / name).open()))

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def percentile(values, p):
    return sorted(values)[math.ceil(len(values) * p / 100) - 1]

manifest = ROOT / 'SHA256SUMS'
if manifest.exists():
    for line in manifest.read_text().splitlines():
        expected, name = line.split('  ', 1)
        assert digest(ROOT / name) == expected, name
state = load('state.json')
checks = load('checks.json')
assert len(checks) == 12 and all(c['returncode'] == 0 for c in checks)
source_hash = hashlib.sha256(json.dumps(state['production_sha256'], sort_keys=True).encode()).hexdigest()
assert all(c['source_snapshot_sha256'] == source_hash for c in checks)
assert load('code-review-final-exit.json')['returncode'] == 0
product = load('candidate-product/environment.json')
baseline = load('baseline-provenance.json')
assert product['benchmark_sources'] == baseline['benchmark_sources']
assert product['imgui_sources'] == baseline['imgui_sources']
changed = sorted(p for p in product['dockspace_sources']
                 if product['dockspace_sources'][p] != baseline['sources'][p])
assert changed == sorted(state['production_scope'])
assert all(p.startswith('crates/egui_dockspace/') for p in changed)
assert all(product['dockspace_sources'][p] == h for p, h in state['production_sha256'].items())

work_fields = ['panes', 'scenario', 'sample', 'frames', 'passes', 'content_calls',
               'visible_contents', 'vertices', 'indices', 'draw_commands']

def verify_product(directory, grouped):
    raw = rows(directory + '/raw.csv')
    summaries = rows(directory + '/summary.csv')
    metadata = load(directory + '/environment.json')
    keys = [(str(g), c) for g in [1, 2] for c in ['baseline', 'candidate']] if grouped else [
        ('egui_dockspace',), ('dear_imgui',)]
    for key in keys:
        selected = [r for r in raw if ((r['group'], r['condition']) if grouped
                                      else (r['product'],)) == key]
        for panes in ['16', '128', '1024']:
            for case, frames in [('idle', 1), ('tab_click', 2), ('surface_resize', 1)]:
                samples = [r for r in selected if (r['panes'], r['scenario']) == (panes, case)]
                assert len(samples) == 48
                assert Counter(int(r['sample']) for r in samples) == Counter({n: 2 for n in range(24)})
                for r in samples:
                    assert int(r['tabs_per_pane']) == 2 and int(r['frames']) == frames
                    assert int(r['visible_contents']) == int(panes) * int(r['passes'])
                    assert int(r['content_calls']) >= int(r['visible_contents'])
                    assert all(int(r[k]) > 0 for k in ['elapsed_ns', 'vertices', 'indices', 'draw_commands'])
                summary = [r for r in summaries if ((r['group'], r['condition']) if grouped
                           else (r['product'],)) == key and (r['panes'], r['scenario']) == (panes, case)]
                assert len(summary) == 1
                values = [int(r['elapsed_ns']) for r in samples]
                assert int(summary[0]['samples']) == 48
                for field, value in [('p50_ns', percentile(values, 50)),
                                     ('p95_ns', percentile(values, 95)), ('max_ns', max(values))]:
                    assert int(summary[0][field]) == value
    if grouped:
        for group in ['1', '2']:
            work = {c: Counter(tuple(r[k] for k in work_fields) for r in raw
                               if r['group'] == group and r['condition'] == c)
                    for c in ['baseline', 'candidate']}
            assert work['baseline'] == work['candidate']
    for name, expected in metadata['results_sha256'].items():
        assert digest(ROOT / directory / name) == expected
    return len(raw)

count = verify_product('frozen-product', True)
count += verify_product('candidate-product', False)
pilot = verify_product('title-reuse-only/frozen-product', True)
pilot += verify_product('title-reuse-only/candidate-product', False)

# Only CPU samples from the ordinary production executables qualify latency.
formal = load('frozen-product/environment.json')['runs']
formal += load('candidate-product/environment.json')['runs']
activities = [(x['check'], x['started_utc'], x['finished_utc']) for x in checks]
for name in ['code-review-exit.json', 'code-review-followup-exit.json', 'code-review-final-exit.json']:
    r = load(name)
    activities.append((name, r['started_utc'], r['finished_utc']))
for label in ['baseline', 'candidate']:
    for case in ['idle', 'tab_click']:
        r = load(f'{label}-{case}.json')
        assert r['returncode'] == r['sampler_returncode'] == 0
        activities.append((f'{label}-{case}-profile', r['started_utc'], r['finished_utc']))
for name in ['candidate-product', 'title-reuse-only/candidate-product']:
    m = load(name + '/environment.json')
    for kind in ['rust', 'cpp']:
        activities.append((f'{name}-{kind}', m[f'{kind}_build_started_utc'], m[f'{kind}_build_finished_utc']))
alloc_receipts = load('allocation-commands.json')
assert len(alloc_receipts) == 8 and all(r['returncode'] == 0 for r in alloc_receipts)
for r in alloc_receipts:
    activities.append(('active-DHAT' if 'run' in r else 'allocation-' + r['step'],
                       r['started_utc'], r['finished_utc']))
for run in formal:
    # compare_products names these fields started_utc/finished_utc.
    start, end = datetime.fromisoformat(run['started_utc']), datetime.fromisoformat(run['finished_utc'])
    for name, begin, finish in activities:
        assert end <= datetime.fromisoformat(begin) or datetime.fromisoformat(finish) <= start, name

allocation_summary = []
allocation_work = {}
for run, label in enumerate(['baseline', 'candidate', 'candidate', 'baseline'], 1):
    raw = rows(f'allocation-run-{run}-{label}.csv')
    assert len(raw) == 54
    expected = {(p, c, str(i)) for p in ['16', '128', '1024']
                for c in ['idle', 'tab_click', 'surface_resize'] for i in range(6)}
    assert {(r['panes'], r['scenario'], r['sample']) for r in raw} == expected
    allocation_work[run] = Counter(tuple(r[k] for k in work_fields) for r in raw)
    for panes in ['16', '128', '1024']:
        for case in ['idle', 'tab_click', 'surface_resize']:
            selected = [r for r in raw if (r['panes'], r['scenario']) == (panes, case)]
            for metric in ['allocation_events', 'requested_bytes', 'live_before_bytes',
                           'live_after_bytes', 'case_prefix_peak_bytes']:
                values = [int(r[metric]) for r in selected]
                allocation_summary.append(dict(run=run, condition=label, panes=panes,
                    scenario=case, metric=metric, samples=6, p50=percentile(values, 50),
                    minimum=min(values), maximum=max(values)))
assert allocation_work[1] == allocation_work[2] == allocation_work[3] == allocation_work[4]
assert (ROOT / 'allocation-baseline/Cargo.lock').read_bytes() == (ROOT / 'allocation-candidate/Cargo.lock').read_bytes()
provenance = load('allocation-provenance.json')
assert provenance['baseline_source_matches_commit']
assert provenance['instrumented_main_sha256'] == digest(ROOT / 'allocation-baseline/src/main.rs')
assert provenance['instrumented_main_sha256'] == digest(ROOT / 'allocation-candidate/src/main.rs')
assert provenance['lock_sha256'] == provenance['candidate_lock_sha256'] == digest(ROOT / 'allocation-baseline/Cargo.lock')
assert sorted(p for p in provenance['baseline_sources']
              if provenance['baseline_sources'][p] != provenance['candidate_sources'][p]) == changed
for run in [1, 2, 3, 4]:
    source_rows = rows(f'allocation-run-{run}-' + ('baseline' if run in [1, 4] else 'candidate') + '.csv')
    boundary = Counter(tuple(r[k] for k in ['panes', 'scenario', 'sample',
                       'live_before_bytes', 'live_after_bytes', 'case_prefix_peak_bytes']) for r in source_rows)
    if run == 1:
        baseline_boundary = boundary
    else:
        assert boundary == baseline_boundary

assert list(csv.DictReader((ROOT / 'allocation-summary.csv').open())) == [
    {k: str(v) for k, v in r.items()} for r in allocation_summary]

# The goal is specifically a complete 1,024-pane idle median improvement in both groups.
summary = rows('frozen-product/summary.csv')
for group in ['1', '2']:
    selected = {r['condition']: int(r['p50_ns']) for r in summary
                if (r['group'], r['panes'], r['scenario']) == (group, '1024', 'idle')}
    assert selected['candidate'] <= selected['baseline'] * 0.95
print(f'Verified {count} final raw CPU rows, {pilot} first-candidate rows, '
      '216 separate allocation rows, exact work, percentiles, adapter-only scope and timing separation.')
