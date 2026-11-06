"""Check the observed idle regression using unchanged frozen executables."""

from datetime import datetime, timezone
from pathlib import Path
import argparse
import csv
import hashlib
import json
import math
import subprocess
import sys

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('output')
parser.add_argument('--samples',type=int,default=24)
parser.add_argument('--warmup',type=int,default=6)
parser.add_argument('--panes',default='16,128,1024')
args=parser.parse_args()
panes=[int(value) for value in args.panes.split(',')]
root = Path(__file__).resolve().parent
previous = Path(json.loads((root/'state.json').read_text())['previous_experiment'])
output = root/args.output
output.mkdir(exist_ok=False)
binaries = {label:previous/f'{label}-product-egui_dockspace' for label in ['baseline','candidate']}
hashes = {label:hashlib.sha256(path.read_bytes()).hexdigest() for label,path in binaries.items()}
expected = json.loads((previous/'frozen-product'/'environment.json').read_text())['binary_sha256']
assert hashes == expected
metadata = {'samples':args.samples,'warmup':args.warmup,'panes':panes,'binary_sha256':hashes,'runs':[]}
rows = []
for group,order in [(1,['baseline','candidate','candidate','baseline']),(2,['candidate','baseline','baseline','candidate'])]:
    for run,label in enumerate(order,1):
        command = [str(binaries[label]),'--samples',str(args.samples),'--warmup',str(args.warmup),'--panes',args.panes]
        prefix = output/f'group-{group}-run-{run}-{label}'
        item = {'group':group,'run':run,'condition':label,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
        print('Reproduce',group,run,label,flush=True)
        result = subprocess.run(command,text=True,capture_output=True)
        prefix.with_suffix('.csv').write_text(result.stdout)
        prefix.with_suffix('.stderr').write_text(result.stderr)
        item.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat())
        metadata['runs'].append(item)
        (output/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
        assert result.returncode == 0
        samples = list(csv.DictReader(result.stdout.splitlines()))
        assert len(samples)==len(panes)*3*args.samples
        assert {(r['panes'],r['scenario'],r['sample']) for r in samples}=={(str(pane),case,str(n)) for pane in panes for case in ['idle','tab_click','surface_resize'] for n in range(args.samples)}
        rows.extend(dict(group=group,run=run,condition=label,**r) for r in samples)
with (output/'raw.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(rows[0]),lineterminator='\n')
    writer.writeheader();writer.writerows(rows)
summaries=[]
for group in [1,2]:
    for label in ['baseline','candidate']:
        for case in ['idle','tab_click','surface_resize']:
            selected=[r for r in rows if (r['group'],r['condition'],r['panes'],r['scenario'])==(group,label,'1024',case)]
            values=sorted(int(r['elapsed_ns']) for r in selected)
            summaries.append(dict(group=group,condition=label,scenario=case,samples=len(values),p50_ns=values[math.ceil(len(values)*.5)-1],p95_ns=values[math.ceil(len(values)*.95)-1],max_ns=values[-1]))
with (output/'summary.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(summaries[0]),lineterminator='\n')
    writer.writeheader();writer.writerows(summaries)
deltas=[]
for group in [1,2]:
    before=next(r['p50_ns'] for r in summaries if (r['group'],r['condition'],r['scenario'])==(group,'baseline','idle'))
    after=next(r['p50_ns'] for r in summaries if (r['group'],r['condition'],r['scenario'])==(group,'candidate','idle'))
    deltas.append((after/before-1)*100)
    print('Idle group',group,'before',before,'after',after,'change_percent',round(deltas[-1],3),flush=True)
metadata.update(idle_changes_percent=deltas,regression_reproduced=all(delta>1 for delta in deltas),threshold='both independent order groups have >1% higher idle p50; diagnostic signal, not a CI gate')
(output/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
raise SystemExit(10 if metadata['regression_reproduced'] else 0)
