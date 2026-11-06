"""Independently check archived samples, percentiles, coverage and equal work."""

from collections import defaultdict
from pathlib import Path
import csv
import hashlib
import json
import math
import sys

base = Path(sys.argv[1])

def read(path):
    with path.open(newline='') as stream:
        return list(csv.DictReader(stream))

def percentile(rows, column, percent):
    values = sorted(int(row[column]) for row in rows)
    return values[math.ceil(len(values) * percent / 100) - 1]

verification = {'datasets': {}, 'geometry_digests': {}, 'workload_equality': {}}
geometries = defaultdict(set)
product_work = {}
for name in ['baseline-layout', 'candidate-layout', 'baseline-product', 'candidate-product', 'frozen-layout', 'frozen-product', 'frozen-product-reverse']:
    root = base / name
    environment = json.loads((root / 'environment.json').read_text())
    rows = read(root / 'raw.csv')
    is_layout = name.endswith('layout')
    frozen = name.startswith('frozen')
    case_key = 'case' if is_layout else 'scenario'
    cases = ['project_full', 'project_prepared'] if is_layout else ['idle', 'tab_click', 'surface_resize']
    fields = [key for key in ['group', 'condition' if frozen else 'product', 'panes', case_key] if key in rows[0]]
    grouped = defaultdict(list)
    processes = defaultdict(list)
    for row in rows:
        grouped[tuple(row[key] for key in fields)].append(row)
        process = (row.get('group', ''), row['run'], row['condition' if frozen else 'product'])
        processes[process].append(row)
        assert int(row['elapsed_ns']) > 0
        if is_layout:
            count = int(row['panes'])
            assert (int(row['nodes']),int(row['leaves']),int(row['splits'])) == (count*2-1,count,count-1)
            assert int(row['height']) == 16380
            assert int(row['width']) == (16380,16444)[(int(row['sample'])+environment['warmup'])%2]
            geometries[row['panes'],row['width']].add(row['geometry_digest'])
    assert len(processes) == len(environment['runs'])
    for process in environment['runs']:
        label = process['condition' if frozen else 'product']
        key = (str(process.get('group','')),str(process['run']),label)
        actual = processes[key]
        process_cases = cases if frozen or label != 'dear_imgui' or not is_layout else ['project_in_place']
        expected = {(str(n),case,str(i)) for n in environment['panes'] for case in process_cases for i in range(environment['samples'])}
        observed = {(r['panes'],r[case_key],r['sample']) for r in actual}
        assert observed == expected and len(actual) == len(expected), (name,key)
        prefix = f"group-{process['group']}-run-{process['run']}-{label}" if 'group' in process else f"run-{process['run']}-{label}"
        raw_process = read(root / (prefix+'.csv'))
        assert [{k:v for k,v in r.items() if k not in ['group','run','condition']} for r in actual] == raw_process
        assert process['returncode'] == 0
    summaries = read(root / 'summary.csv')
    assert len(summaries) == len(grouped)
    for summary in summaries:
        values = grouped[tuple(summary['case' if frozen and key==case_key else key] for key in fields)]
        assert int(summary['samples']) == len(values)
        for percent in [50,95]:
            assert int(summary[f'p{percent}_ns']) == percentile(values,'elapsed_ns',percent)
        assert int(summary['max_ns']) == max(int(row['elapsed_ns']) for row in values)
        if 'preparation_p50_ns' in summary:
            assert int(summary['preparation_p50_ns']) == percentile(values,'preparation_ns',50)
        for metric in ['passes','content_calls','visible_contents','vertices','indices','draw_commands']:
            if 'p50_'+metric in summary:
                assert int(summary['p50_'+metric]) == percentile(values,metric,50)
    for filename, expected_hash in environment.get('results_sha256',{}).items():
        assert hashlib.sha256((root/filename).read_bytes()).hexdigest() == expected_hash
    if not is_layout:
        work = defaultdict(set)
        metrics = ['tabs_per_pane','frames','passes','content_calls','visible_contents','vertices','indices','draw_commands']
        for row in rows:
            assert int(row['frames']) == (2 if row['scenario']=='tab_click' else 1)
            assert int(row['visible_contents']) == int(row['panes'])*int(row['passes'])
            work[row['product'],row['panes'],row['scenario'],row['sample']].add(tuple(row[m] for m in metrics))
        if frozen:
            for label in ['baseline','candidate']:
                subset = defaultdict(set)
                for row in rows:
                    if row['condition']==label:
                        subset[row['product'],row['panes'],row['scenario'],row['sample']].add(tuple(row[m] for m in metrics))
                assert subset == {k:v for k,v in product_work['baseline-product'].items() if k[0]=='egui_dockspace'}
        else:
            product_work[name] = dict(work)
    verification['datasets'][name] = {'rows':len(rows),'processes':len(processes),'summaries':len(summaries),'coverage_and_nearest_rank_percentiles':'pass'}

assert product_work['baseline-product'] == product_work['candidate-product']
assert all(len(values)==1 for values in geometries.values())
verification['geometry_digests'] = {','.join(key):next(iter(values)) for key,values in sorted(geometries.items())}
verification['workload_equality'] = {'all_product_samples': 'pass', 'metrics':metrics}
allowed = set(json.loads((base/'state.json').read_text())['source_scope'])
for suite, source_key in [('layout','sources'),('product','dockspace_sources')]:
    old = json.loads((base/f'baseline-{suite}'/'environment.json').read_text())
    new = json.loads((base/f'candidate-{suite}'/'environment.json').read_text())
    changed = {name for name in old[source_key] if old[source_key][name] != new[source_key].get(name)}
    added = set(new[source_key])-set(old[source_key])
    assert changed | added <= allowed, (changed,added)
    assert old['imgui_sources'] == new['imgui_sources']
    verification[suite+'_source_changes'] = {'changed':sorted(changed),'added':sorted(added)}

headless = base/'headless-investigation'
meta = json.loads((headless/'environment.json').read_text())
state = json.loads((base/'state.json').read_text())
assert all(state['source_inventory'].get(n)==h for n,h in meta['baseline_environment']['sources'].items())
assert meta['candidate_sources']==state['source_current_sha256']
summaries = read(headless/'process-summaries.csv')
assert len(summaries)==80 and len(meta['runs'])==8
for run in meta['runs']:
    selected=[r for r in summaries if (r['group'],r['run'],r['condition'])==(str(run['group']),str(run['run']),run['condition'])]
    process=read(headless/f"group-{run['group']}-run-{run['run']}-{run['condition']}.csv")
    assert len(selected)==10
    assert [{k:v for k,v in r.items() if k not in ['group','run','condition']} for r in selected]==process
    assert len({(r['scenario'],r['phase']) for r in selected})==10
    assert all(r['metric']=='time_ns' and r['instrumentation']=='dhat_inactive' and r['samples']=='48' and r['warmup']=='6' for r in selected)
for row in summaries:
    assert int(row['p50'])<=int(row['p95'])<=int(row['max'])
for run in meta['allocation_runs']:
    rows=read(headless/f"allocations-{run['condition']}.csv")
    assert len(rows)==60 and len({(r['scenario'],r['phase'],r['metric']) for r in rows})==60
    assert all(r['samples']=='3' and r['warmup']=='1' for r in rows)
for filename,expected in meta['results_sha256'].items():
    assert hashlib.sha256((headless/filename).read_bytes()).hexdigest()==expected
verification['headless_attribution']={'process_summaries':80,'allocation_and_time_summaries':120,'baseline_matching_source_files':len(meta['baseline_environment']['sources']),'checks':'pass; per-sample timings are not exposed by this unchanged harness'}
(base/'verification.json').write_text(json.dumps(verification,indent=2)+'\n')
print(json.dumps(verification,indent=2))
