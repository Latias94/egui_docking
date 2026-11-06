"""Compare frozen before/after executables with the unchanged benchmark workloads."""

from datetime import datetime, timezone
from pathlib import Path
import csv
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path.cwd()
BASE = Path(sys.argv[1])
sys.path.insert(0, str(ROOT / 'scripts'))
from compare_layouts import validate as validate_layout
from compare_products import validate_rows as validate_product

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def now():
    return datetime.now(timezone.utc).isoformat()

checks = json.loads((BASE / 'checks.json').read_text())
assert len(checks) == 10 and all(row['returncode'] == 0 for row in checks)
assert json.loads((BASE / 'review-exit.json').read_text())['returncode'] == 0
state = json.loads((BASE / 'state.json').read_text())
sources = {name: sha(ROOT / name) for name in state['source_scope']}
assert sources == state['source_current_sha256']

for suite, product in [('layout', 'dockspace'), ('product', 'egui_dockspace')]:
    binaries = {label: BASE / f'{label}-{suite}-{product}' for label in ['baseline', 'candidate']}
    hashes = {label: sha(path) for label, path in binaries.items()}
    for label in binaries:
        expected = json.loads((BASE / f'{label}-{suite}' / 'environment.json').read_text())
        assert hashes[label] == expected['binary_sha256'][product]
    output = BASE / f'frozen-{suite}'
    output.mkdir(exist_ok=False)
    metadata = {
        'started_utc': now(), 'scope': 'Unchanged benchmark, frozen before/after executables; two serial ABBA groups.',
        'base_commit': state['base'], 'candidate_sources': sources,
        'binary_sha256': hashes, 'panes': [16, 128, 1024],
        'samples': 96 if suite == 'layout' else 24,
        'warmup': 16 if suite == 'layout' else 6, 'runs': [],
    }
    rows = []
    for group in [1, 2]:
        for run, label in enumerate(['baseline', 'candidate', 'candidate', 'baseline'], 1):
            prefix = output / f'group-{group}-run-{run}-{label}'
            raw = prefix.with_suffix('.csv')
            command = [str(binaries[label])]
            environment = dict(os.environ)
            if suite == 'layout':
                command += ['--exact', 'layout::layout_cost::layout_cost', '--ignored', '--nocapture', '--test-threads', '1']
                environment.update(DOCKSPACE_LAYOUT_COST_SAMPLES='96', DOCKSPACE_LAYOUT_COST_WARMUP='16',
                                   DOCKSPACE_LAYOUT_COST_PANES='16,128,1024', DOCKSPACE_LAYOUT_COST_OUT=str(raw))
            else:
                command += ['--samples', '24', '--warmup', '6', '--panes', '16,128,1024']
            item = {'group': group, 'run': run, 'condition': label, 'command': command, 'started_utc': now()}
            print(suite, 'group', group, 'run', run, label, flush=True)
            result = subprocess.run(command, cwd=ROOT, env=environment, text=True, capture_output=True)
            prefix.with_suffix('.stdout').write_text(result.stdout)
            prefix.with_suffix('.stderr').write_text(result.stderr)
            if suite == 'product':
                raw.write_text(result.stdout)
            item.update(returncode=result.returncode, finished_utc=now())
            metadata['runs'].append(item)
            (output / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
            assert result.returncode == 0, prefix
            if suite == 'layout':
                samples = validate_layout(raw.read_text(), product, [16,128,1024], 96, 16)
            else:
                samples = validate_product(raw.read_text(), product, [16,128,1024], 24)
            rows.extend(dict(group=group, run=run, condition=label, **row) for row in samples)
    with (output / 'raw.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]), lineterminator='\n')
        writer.writeheader()
        writer.writerows(rows)
    summaries = []
    case_key = 'case' if suite == 'layout' else 'scenario'
    cases = ['project_full', 'project_prepared'] if suite == 'layout' else ['idle', 'tab_click', 'surface_resize']
    for group in [1,2]:
        for label in ['baseline', 'candidate']:
            for panes in [16,128,1024]:
                for case in cases:
                    selected = [r for r in rows if (r['group'],r['condition'],r['panes'],r[case_key]) == (group,label,panes,case)]
                    values = sorted(r['elapsed_ns'] for r in selected)
                    summaries.append(dict(group=group, condition=label, panes=panes, case=case,
                                          samples=len(values), p50_ns=values[(len(values)+1)//2-1],
                                          p95_ns=values[(len(values)*95+99)//100-1], max_ns=values[-1]))
    with (output / 'summary.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=list(summaries[0]), lineterminator='\n')
        writer.writeheader()
        writer.writerows(summaries)
    assert hashes == {label: sha(path) for label, path in binaries.items()}
    assert sources == {name: sha(ROOT / name) for name in sources}
    metadata.update(finished_utc=now(), input_hashes_unchanged_after_run=True,
                    results_sha256={name:sha(output/name) for name in ['raw.csv','summary.csv']})
    (output / 'environment.json').write_text(json.dumps(metadata, indent=2) + '\n')
print('Frozen comparisons complete', flush=True)
