"""Recompute experiment sample coverage, equal work and stage percentiles."""

from collections import defaultdict
from pathlib import Path
import csv
import hashlib
import json
import math
import sys

root=Path(sys.argv[1])

def read(path):
    with path.open(newline='') as stream:return list(csv.DictReader(stream))

verified={}
for name in ['reproduction-one','reproduction-original','imgui-assert-comparison','validation-frozen-product']:
    path=root/name;metadata=json.loads((path/'environment.json').read_text());rows=read(path/'raw.csv')
    workload={}
    for process in metadata['runs']:
        selected=[r for r in rows if (r['group'],r['run'],r['condition'])==(str(process['group']),str(process['run']),process['condition'])]
        expected={(str(panes),case,str(n)) for panes in metadata['panes'] for case in ['idle','tab_click','surface_resize'] for n in range(metadata['samples'])}
        assert {(r['panes'],r['scenario'],r['sample']) for r in selected}==expected and len(selected)==len(expected)
        raw=read(path/f"group-{process['group']}-run-{process['run']}-{process['condition']}.csv")
        assert [{k:v for k,v in r.items() if k not in ['group','run','condition']} for r in selected]==raw
        assert process['returncode']==0
    for row in rows:
        key=row['panes'],row['scenario'],row['sample']
        counts=tuple(row[name] for name in ['tabs_per_pane','frames','passes','content_calls','visible_contents','vertices','indices','draw_commands'])
        assert key not in workload or workload[key]==counts
        workload[key]=counts
    for summary in read(path/'summary.csv'):
        values=sorted(int(r['elapsed_ns']) for r in rows if (r['panes'],r['group'],r['condition'],r['scenario'])==(summary.get('panes','1024'),summary['group'],summary['condition'],summary['scenario']))
        assert int(summary['samples'])==len(values)
        assert int(summary['p50_ns'])==values[math.ceil(len(values)*.5)-1]
        assert int(summary['p95_ns'])==values[math.ceil(len(values)*.95)-1]
        assert int(summary['max_ns'])==values[-1]
    verified[name]={'rows':len(rows),'processes':len(metadata['runs']),'coverage_and_workload_and_percentiles':'pass'}
    for filename,expected in metadata.get('results_sha256',{}).items():
        assert hashlib.sha256((path/filename).read_bytes()).hexdigest()==expected

candidate=root/'validation-candidate-product'
meta=json.loads((candidate/'environment.json').read_text())
rows=read(candidate/'raw.csv')
for run in meta['runs']:
    values=[r for r in rows if (r['run'],r['product'])==(str(run['run']),run['product'])]
    expected={(str(n),case,str(i)) for n in meta['panes'] for case in ['idle','tab_click','surface_resize'] for i in range(meta['samples'])}
    assert {(r['panes'],r['scenario'],r['sample']) for r in values}==expected and len(values)==len(expected)
    process=read(candidate/f"run-{run['run']}-{run['product']}.csv")
    assert [{k:v for k,v in r.items() if k!='run'} for r in values]==process
    assert run['returncode']==0
for summary in read(candidate/'summary.csv'):
    values=[r for r in rows if (r['product'],r['panes'],r['scenario'])==(summary['product'],summary['panes'],summary['scenario'])]
    times=sorted(int(r['elapsed_ns']) for r in values)
    assert int(summary['samples'])==len(values)
    for percent in [50,95]:assert int(summary[f'p{percent}_ns'])==times[math.ceil(len(times)*percent/100)-1]
    assert int(summary['max_ns'])==times[-1]
    for metric in ['passes','content_calls','visible_contents','vertices','indices','draw_commands']:
        counts=sorted(int(r[metric]) for r in values)
        assert int(summary['p50_'+metric])==counts[math.ceil(len(counts)*.5)-1]
for filename,expected in meta['results_sha256'].items():
    assert hashlib.sha256((candidate/filename).read_bytes()).hexdigest()==expected
frozen_meta=json.loads((root/'validation-frozen-product'/'environment.json').read_text())
assert frozen_meta['candidate_sources']==meta['dockspace_sources']
changed={n for n,h in frozen_meta['baseline_sources'].items() if meta['dockspace_sources'].get(n)!=h}
assert changed=={'crates/dockspace/src/operation.rs','crates/dockspace/src/canonical.rs'}
verified['validation-candidate-product']={'rows':len(rows),'processes':len(meta['runs']),'coverage_and_percentiles':'pass','only_changed_sources':sorted(changed)}

headless=root/'validation-headless'
meta=json.loads((headless/'environment.json').read_text())
rows=read(headless/'process-summaries.csv')
assert len(meta['runs'])==8 and len(rows)==240
cases=['idle','select','drag','measurement_change','outputs_in_flight']
for run in meta['runs']:
    values=[r for r in rows if (r['group'],r['run'],r['condition'])==(str(run['group']),str(run['run']),run['condition'])]
    assert len(values)==30
    assert {(r['panes'],r['scenario'],r['phase']) for r in values}=={(str(n),case,phase) for n in [16,128,1024] for case in cases for phase in ['update','settlement']}
    assert all(r['samples']=='48' and r['warmup']=='6' and r['instrumentation']=='dhat_inactive' and r['metric']=='time_ns' for r in values)
    process=read(headless/f"group-{run['group']}-run-{run['run']}-{run['condition']}.csv")
    assert [{k:v for k,v in r.items() if k not in ['group','run','condition']} for r in values]==process
    assert run['returncode']==0
for run in meta['allocation_runs']:
    values=read(headless/f"allocations-{run['condition']}.csv")
    assert run['returncode']==0 and len(values)==180
    assert len({(r['panes'],r['scenario'],r['phase'],r['metric']) for r in values})==180
    assert all(r['samples']=='6' and r['warmup']=='2' for r in values)
    for row in values:
        assert int(row['p50'])<=int(row['p95'])<=int(row['max'])
for filename,expected in meta['results_sha256'].items():
    assert hashlib.sha256((headless/filename).read_bytes()).hexdigest()==expected
verified['validation-headless']={'process_summaries':len(rows),'timing_processes':8,'allocation_processes':2,'checks':'pass; unchanged harness exposes process summaries, not raw timing samples'}

grouped=defaultdict(list)
for row in read(root/'stages-raw.csv'):
    grouped[row['scenario'],row['panes'],row['span']].append(row)
for summary in read(root/'stages-summary.csv'):
    values=grouped[summary['scenario'],summary['panes'],summary['span']]
    assert int(summary['samples'])==len(values)
    times=sorted(int(r['elapsed_ns']) for r in values)
    for percent in [50,95]:assert int(summary[f'p{percent}_ns'])==times[math.ceil(len(times)*percent/100)-1]
    assert int(summary['max_ns'])==times[-1]
    assert int(summary['calls_min'])==min(int(r['calls']) for r in values)
    assert int(summary['calls_max'])==max(int(r['calls']) for r in values)
verified['stages']={'stage_groups':len(grouped),'coverage':json.loads((root/'stages-verification.json').read_text())['coverage'],'nearest_rank_percentiles_and_call_counts':'pass','limits':'Instrumented times are diagnostic. Nested spans overlap; stages are not independently removable work.'}
(root/'verification.json').write_text(json.dumps(verified,indent=2)+'\n')
print(json.dumps(verified,indent=2))
