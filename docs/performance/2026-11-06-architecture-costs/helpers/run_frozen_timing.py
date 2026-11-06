from pathlib import Path
from datetime import datetime, timezone
import csv, hashlib, json, subprocess
base = Path('/tmp/dockspace-architecture-20261106-0a1dcf8')
candidate = base / 'candidate-manifest'
conditions = {'baseline': base, 'shared-manifest': candidate}
for label, root in conditions.items():
    meta = json.loads((root / 'environment.json').read_text())
    assert meta['returncode'] == 0 and hashlib.sha256((root / 'frame_cost').read_bytes()).hexdigest() == meta['binary_sha256']
output = base / 'frozen-timing'
output.mkdir(exist_ok=False)
metadata = {'scope': 'Timing-only complete existing headless workload, no active DHAT; two ABBA groups. Original harness exposes per-process p50/p95/max, not individual timings. No pooling of medians.', 'samples_per_process_case': 48, 'warmup': 6, 'in_flight': 1, 'panes': [16,128,1024], 'runs': []}
rows = []
for group in [1,2]:
    for run, label in enumerate(['baseline','shared-manifest','shared-manifest','baseline'],1):
        binary = conditions[label] / 'frame_cost'
        command = [str(binary),'--timing-only','--samples','48','--warmup','6','--in-flight','1','--panes','16,128,1024']
        prefix = output / f'group-{group}-run-{run}-{label}'
        item = {'group': group, 'run': run, 'condition': label, 'command': command, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'started_utc': datetime.now(timezone.utc).isoformat()}
        print('Timing group',group,'run',run,label,flush=True)
        with prefix.with_suffix('.csv').open('w') as out, prefix.with_suffix('.stderr').open('w') as err:
            result = subprocess.run(command, stdout=out, stderr=err)
        item['returncode'] = result.returncode
        item['finished_utc'] = datetime.now(timezone.utc).isoformat()
        metadata['runs'].append(item)
        (output / 'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
        assert result.returncode == 0, prefix.with_suffix('.stderr').read_text()[-4000:]
        with prefix.with_suffix('.csv').open(newline='') as f: samples = list(csv.DictReader(f))
        assert len(samples) == 30 and all(r['metric'] == 'time_ns' and r['instrumentation'] == 'dhat_inactive' for r in samples)
        rows.extend(dict(group=group,run=run,condition=label,**r) for r in samples)
with (output / 'process-summaries.csv').open('w',newline='') as f:
    writer = csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
metadata['finished_utc'] = datetime.now(timezone.utc).isoformat()
metadata['results_sha256'] = {'process-summaries.csv': hashlib.sha256((output/'process-summaries.csv').read_bytes()).hexdigest()}
(output / 'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
for scenario in ['idle','select','measurement_change']:
    for phase in ['update','settlement']:
        print(scenario,phase,{label:[round(int(r['p50'])/1e6,6) for r in rows if r['panes']=='1024' and r['scenario']==scenario and r['phase']==phase and r['condition']==label] for label in conditions},flush=True)
print('Frozen timing comparisons complete',flush=True)
