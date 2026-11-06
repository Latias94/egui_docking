"""Keep nested spans distinct and summarize complete warm diagnostic updates."""

from collections import defaultdict
from pathlib import Path
import csv
import gzip
import json
import math
import sys

root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent
updates=[]
current=None
trace=(root/'stages.stderr').read_text() if (root/'stages.stderr').exists() else gzip.decompress((root/'stages.stderr.gz').read_bytes()).decode()
for line in trace.splitlines():
    if not line.startswith('[DEBUG-select-cost] '):
        continue
    fields=line.split()[1:]
    if fields[0]=='begin_update':
        assert current is None
        current={'scenario':fields[1],'panes':int(fields[2]),'step':int(fields[3]),'values':defaultdict(int),'calls':defaultdict(int)}
    elif current is not None and fields[0] in ['part','span']:
        key=':'.join(fields[:-1])
        current['values'][key]+=int(fields[-1])
        current['calls'][key]+=1
        if fields[:2]==['span','harness_update']:
            updates.append(current)
            current=None
assert current is None

rows=[]
for update in updates:
    if update['step']<7 or update['scenario'] not in ['idle','select','measurement_change']:
        continue
    for name,value in update['values'].items():
        rows.append(dict(scenario=update['scenario'],panes=update['panes'],step=update['step'],span=name,elapsed_ns=value,calls=update['calls'][name]))
with (root/'stages-raw.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
grouped=defaultdict(list)
for row in rows:
    grouped[row['scenario'],row['panes'],row['span']].append(row)
summaries=[]
for (scenario,panes,span),group in sorted(grouped.items()):
    values=sorted(row['elapsed_ns'] for row in group)
    summaries.append(dict(scenario=scenario,panes=panes,span=span,samples=len(group),p50_ns=values[math.ceil(len(values)*.5)-1],p95_ns=values[math.ceil(len(values)*.95)-1],max_ns=values[-1],calls_min=min(row['calls'] for row in group),calls_max=max(row['calls'] for row in group)))
with (root/'stages-summary.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(summaries[0]),lineterminator='\n');writer.writeheader();writer.writerows(summaries)
for scenario in ['idle','select','measurement_change']:
    print(scenario)
    for row in summaries:
        if row['scenario']==scenario and row['panes']==1024 and (row['span'].startswith('span:') or row['span'].startswith('part:harness_update:') or row['span'].startswith('part:transaction:')):
            print(row['span'],round(row['p50_ns']/1e6,6),'ms',row['samples'],'samples','calls',row['calls_min'],row['calls_max'])
coverage={f'{case},{panes}':len([r for r in updates if r['scenario']==case and r['panes']==panes and r['step']>=7]) for case in ['idle','select','measurement_change'] for panes in [16,128,1024]}
assert all(count==32 for count in coverage.values()),coverage
(root/'stages-verification.json').write_text(json.dumps({'coverage':coverage,'summaries':len(summaries),'raw_span_rows':len(rows),'limits':'Parent spans include child spans and diagnostic printing; do not add nested spans or interpret these as uninstrumented latency qualification. Absent spans represent skipped work, not a measured duration of zero.'},indent=2)+'\n')
