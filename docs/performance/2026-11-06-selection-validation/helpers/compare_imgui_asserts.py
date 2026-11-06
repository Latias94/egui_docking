"""Compare one pinned ImGui build with default assertions enabled/disabled."""

from datetime import datetime,timezone
from pathlib import Path
import csv
import hashlib
import json
import math
import subprocess

root=Path(__file__).resolve().parent
assert json.loads((root/'design-review-exit.json').read_text())['returncode']==0
build=json.loads((root/'imgui-assert-build.json').read_text())
previous=Path(json.loads((root/'state.json').read_text())['previous_experiment'])
binaries={'assertions_disabled':previous/'baseline-product-dear_imgui','assertions_enabled':root/'imgui-assertions-enabled'}
assert all(hashlib.sha256(path.read_bytes()).hexdigest()==build['binary_sha256'][label] for label,path in binaries.items())
output=root/'imgui-assert-comparison';output.mkdir(exist_ok=False)
metadata={'scope':build['scope'],'samples':128,'warmup':16,'panes':[1024],'binary_sha256':build['binary_sha256'],'runs':[]}
rows=[]
for group,order in [(1,['assertions_disabled','assertions_enabled','assertions_enabled','assertions_disabled']),(2,['assertions_enabled','assertions_disabled','assertions_disabled','assertions_enabled'])]:
    for run,label in enumerate(order,1):
        print('ImGui assert comparison',group,run,label,flush=True)
        command=[str(binaries[label]),'--samples','128','--warmup','16','--panes','1024']
        item={'group':group,'run':run,'condition':label,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
        result=subprocess.run(command,text=True,capture_output=True)
        prefix=output/f'group-{group}-run-{run}-{label}'
        prefix.with_suffix('.csv').write_text(result.stdout);prefix.with_suffix('.stderr').write_text(result.stderr)
        item.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat());metadata['runs'].append(item)
        (output/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
        assert result.returncode==0
        samples=list(csv.DictReader(result.stdout.splitlines()));assert len(samples)==384
        assert {(r['scenario'],r['sample']) for r in samples}=={(case,str(i)) for case in ['idle','tab_click','surface_resize'] for i in range(128)}
        rows.extend(dict(group=group,run=run,condition=label,**r) for r in samples)
with (output/'raw.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
summaries=[]
for group in [1,2]:
    for label in binaries:
        for case in ['idle','tab_click','surface_resize']:
            subset=[r for r in rows if (r['group'],r['condition'],r['scenario'])==(group,label,case)]
            values=sorted(int(r['elapsed_ns']) for r in subset)
            summaries.append(dict(group=group,condition=label,scenario=case,samples=len(values),p50_ns=values[math.ceil(len(values)*.5)-1],p95_ns=values[math.ceil(len(values)*.95)-1],max_ns=values[-1]))
for case in ['idle','tab_click','surface_resize']:
    print(case,{label:[round(row['p50_ns']/1e6,6) for row in summaries if row['condition']==label and row['scenario']==case] for label in binaries},flush=True)
work={}
for row in rows:
    key=row['scenario'],row['sample']
    counts=tuple(row[name] for name in ['panes','tabs_per_pane','frames','passes','content_calls','visible_contents','vertices','indices','draw_commands'])
    assert key not in work or work[key]==counts
    work[key]=counts
with (output/'summary.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(summaries[0]),lineterminator='\n');writer.writeheader();writer.writerows(summaries)
assert all(hashlib.sha256(path.read_bytes()).hexdigest()==build['binary_sha256'][label] for label,path in binaries.items())
metadata.update(finished_utc=datetime.now(timezone.utc).isoformat(),workload_counts_identical=True,results_sha256={name:hashlib.sha256((output/name).read_bytes()).hexdigest() for name in ['raw.csv','summary.csv']})
(output/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
