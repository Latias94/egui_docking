from pathlib import Path
from datetime import datetime, timezone
import csv, hashlib, json, os, shutil, subprocess
root = Path('/Users/frankorz/codes/rust/egui_docking')
base = Path('/tmp/dockspace-architecture-20261106-0a1dcf8')
checks = json.loads((base / 'checks.json').read_text())
assert len(checks) == 7 and all(r['returncode'] == 0 for r in checks)
p = base / 'candidate-manifest'
p.mkdir(exist_ok=False)
sha = lambda f: hashlib.sha256(f.read_bytes()).hexdigest()
original = json.loads((base / 'environment.json').read_text())
sources = {n: sha(root / n) for n in original['sources']}
changes = [n for n, h in original['sources'].items() if sources[n] != h]
assert changes == ['crates/dockspace/src/scene_manifest.rs'], changes
meta = dict(original, sources=sources, condition='Share frozen manifest surface inventories; COW popup rewrites', source_differences=changes)
meta.pop('runs', None)
command = original['build_command']
environment = dict(os.environ, **original['build_environment'])
meta['build_started_utc'] = datetime.now(timezone.utc).isoformat()
(p / 'environment.json').write_text(json.dumps(meta, indent=2) + '\n')
print('Building one-variable manifest candidate...', flush=True)
with (p / 'build.log').open('w') as log:
    result = subprocess.run(command, cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT)
assert result.returncode == 0, (p / 'build.log').read_text()[-5000:]
meta['build_finished_utc'] = datetime.now(timezone.utc).isoformat()
executables = set()
for line in (p / 'build.log').read_text().splitlines():
    try: e = json.loads(line)
    except json.JSONDecodeError: continue
    if e.get('reason') == 'compiler-artifact' and e.get('target', {}).get('name') == 'frame_cost' and e.get('executable'):
        executables.add(Path(e['executable']))
assert len(executables) == 1
binary = p / 'frame_cost'
shutil.copy2(executables.pop(), binary)
meta['binary_sha256'] = sha(binary)
meta['command'] = [str(binary), *original['command'][1:]]
meta['started_utc'] = datetime.now(timezone.utc).isoformat()
(p / 'environment.json').write_text(json.dumps(meta, indent=2) + '\n')
print('Measuring unchanged full headless workload and separate DHAT counts...', flush=True)
with (p / 'headless.csv').open('w') as out, (p / 'run.stderr').open('w') as err:
    result = subprocess.run(meta['command'], cwd=root, stdout=out, stderr=err)
meta['returncode'] = result.returncode
meta['finished_utc'] = datetime.now(timezone.utc).isoformat()
assert all(sha(root / n) == h for n, h in sources.items())
meta['sources_unchanged'] = True
meta['csv_sha256'] = sha(p / 'headless.csv')
(p / 'environment.json').write_text(json.dumps(meta, indent=2) + '\n')
assert result.returncode == 0, (p / 'run.stderr').read_text()[-5000:]
with (p / 'headless.csv').open(newline='') as f: rows = list(csv.DictReader(f))
for scenario in ['idle', 'select', 'measurement_change']:
    print(scenario, {r['phase'] + '/' + r['metric']: r['p50'] for r in rows if r['panes'] == '1024' and r['scenario'] == scenario and r['metric'] in {'time_ns','allocation_events','requested_bytes'}}, flush=True)
print('Candidate complete:', p, flush=True)
