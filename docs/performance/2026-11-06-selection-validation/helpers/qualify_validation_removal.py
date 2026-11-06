"""Qualify the one repeated-validator removal on unchanged frozen workloads."""

from datetime import datetime,timezone
from pathlib import Path
import csv
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

root=Path(__file__).resolve().parent
repo=Path.cwd()
previous=Path(json.loads((root/'state.json').read_text())['previous_experiment'])
sys.path.insert(0,str(repo/'scripts'))
from compare_products import validate_rows

while True:
    checks=json.loads((root/'checks.json').read_text()) if (root/'checks.json').exists() else []
    review=json.loads((root/'minimal-review-exit.json').read_text()) if (root/'minimal-review-exit.json').exists() else None
    if len(checks)==10 and all(row['returncode']==0 for row in checks) and review and review['returncode']==0:
        break
    if any(row['returncode'] for row in checks) or review and review['returncode']:
        raise SystemExit('Required checks or minimal independent review failed')
    time.sleep(1)

state=json.loads((root/'state.json').read_text())
assert all(hashlib.sha256((repo/n).read_bytes()).hexdigest()==h for n,h in state['production_sha256'].items())
commands=[]

def run(name,command,environment=None):
    print(name,flush=True)
    item={'name':name,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
    with (root/(name+'.log')).open('w') as log:
        result=subprocess.run(command,cwd=repo,env=dict(os.environ,**(environment or {})),stdout=log,stderr=subprocess.STDOUT)
    item.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat());commands.append(item)
    (root/'qualification-commands.json').write_text(json.dumps(commands,indent=2)+'\n')
    if result.returncode:
        print((root/(name+'.log')).read_text()[-6000:]);raise SystemExit(result.returncode)

run('imgui-assert-qualification',['python3',str(root/'compare_imgui_asserts.py')])
run('candidate-product-build-and-timing',['python3','scripts/compare_products.py','--backend','fork','--profile','production','--samples','24','--warmup','6','--panes','16,128,1024','--runs','4','--out',str(root/'validation-candidate-product')])
metadata=json.loads((root/'validation-candidate-product'/'environment.json').read_text())
candidate=root/'validation-candidate-product-bin'
shutil.copy2(metadata['binary_paths']['egui_dockspace'],candidate)
assert hashlib.sha256(candidate.read_bytes()).hexdigest()==metadata['binary_sha256']['egui_dockspace']
oldmeta=json.loads((previous/'candidate-product'/'environment.json').read_text())
changed={n for n,h in oldmeta['dockspace_sources'].items() if metadata['dockspace_sources'].get(n)!=h}
assert changed==set(state['production_scope']),changed
assert oldmeta['benchmark_sources']==metadata['benchmark_sources'] and oldmeta['imgui_sources']==metadata['imgui_sources']
output=root/'validation-frozen-product';output.mkdir(exist_ok=False)
binaries={'baseline':previous/'candidate-product-egui_dockspace','candidate':candidate}
hashes={label:hashlib.sha256(path.read_bytes()).hexdigest() for label,path in binaries.items()}
assert hashes['baseline']==oldmeta['binary_sha256']['egui_dockspace']
meta={'samples':24,'warmup':6,'panes':[16,128,1024],'binary_sha256':hashes,'baseline_sources':oldmeta['dockspace_sources'],'candidate_sources':metadata['dockspace_sources'],'runs':[]}
rows=[]
for group,order in [(1,['baseline','candidate','candidate','baseline']),(2,['candidate','baseline','baseline','candidate'])]:
    for number,label in enumerate(order,1):
        print('Frozen validator removal',group,number,label,flush=True)
        command=[str(binaries[label]),'--samples','24','--warmup','6','--panes','16,128,1024']
        item={'group':group,'run':number,'condition':label,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
        result=subprocess.run(command,text=True,capture_output=True)
        prefix=output/f'group-{group}-run-{number}-{label}';prefix.with_suffix('.csv').write_text(result.stdout);prefix.with_suffix('.stderr').write_text(result.stderr)
        item.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat());meta['runs'].append(item);(output/'environment.json').write_text(json.dumps(meta,indent=2)+'\n')
        assert result.returncode==0
        rows.extend(dict(group=group,run=number,condition=label,**r) for r in validate_rows(result.stdout,'egui_dockspace',[16,128,1024],24))
with (output/'raw.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
summaries=[]
for group in [1,2]:
    for label in binaries:
        for panes in [16,128,1024]:
            for case in ['idle','tab_click','surface_resize']:
                values=sorted(r['elapsed_ns'] for r in rows if (r['group'],r['condition'],r['panes'],r['scenario'])==(group,label,panes,case))
                summaries.append(dict(group=group,condition=label,panes=panes,scenario=case,samples=len(values),p50_ns=values[(len(values)+1)//2-1],p95_ns=values[(len(values)*95+99)//100-1],max_ns=values[-1]))
with (output/'summary.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(summaries[0]),lineterminator='\n');writer.writeheader();writer.writerows(summaries)
meta['results_sha256']={name:hashlib.sha256((output/name).read_bytes()).hexdigest() for name in ['raw.csv','summary.csv']};meta['finished_utc']=datetime.now(timezone.utc).isoformat();(output/'environment.json').write_text(json.dumps(meta,indent=2)+'\n')

environment={'CARGO_BUILD_JOBS':'1','CARGO_PROFILE_RELEASE_LTO':'thin','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'1'}
run('headless-build',['cargo','build','--manifest-path','integration/dockspace-host-harness/Cargo.toml','--release','--locked','--offline','--example','frame_cost','--message-format=json-render-diagnostics'],environment)
artifacts=[]
for line in (root/'headless-build.log').read_text().splitlines():
    try:event=json.loads(line)
    except json.JSONDecodeError:continue
    if event.get('reason')=='compiler-artifact' and event.get('target',{}).get('name')=='frame_cost' and event.get('executable'):artifacts.append(Path(event['executable']))
assert len(artifacts)==1
shutil.copy2(artifacts[0],root/'validation-candidate-headless-bin')
output=root/'validation-headless';output.mkdir(exist_ok=False)
binaries={'baseline':previous/'candidate-headless-frame_cost','candidate':root/'validation-candidate-headless-bin'}
hashes={label:hashlib.sha256(path.read_bytes()).hexdigest() for label,path in binaries.items()}
oldhead=json.loads((previous/'headless-investigation'/'environment.json').read_text());assert hashes['baseline']==oldhead['binary_sha256']['candidate']
meta={'scope':'Unchanged full public frame_cost workload. Process summaries only; inactive DHAT timing separate from active allocation accounting.','build_environment':environment,'baseline_environment':oldhead,'production_sha256':state['production_sha256'],'binary_sha256':hashes,'runs':[]};rows=[]
for group,order in [(1,['baseline','candidate','candidate','baseline']),(2,['candidate','baseline','baseline','candidate'])]:
    for number,label in enumerate(order,1):
        print('Headless validator removal',group,number,label,flush=True)
        command=[str(binaries[label]),'--timing-only','--samples','48','--warmup','6','--in-flight','1','--panes','16,128,1024'];prefix=output/f'group-{group}-run-{number}-{label}'
        item={'group':group,'run':number,'condition':label,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
        result=subprocess.run(command,text=True,capture_output=True);prefix.with_suffix('.csv').write_text(result.stdout);prefix.with_suffix('.stderr').write_text(result.stderr)
        item.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat());meta['runs'].append(item);(output/'environment.json').write_text(json.dumps(meta,indent=2)+'\n');assert result.returncode==0
        samples=list(csv.DictReader(result.stdout.splitlines()));assert len(samples)==30
        rows.extend(dict(group=group,run=number,condition=label,**r) for r in samples)
with (output/'process-summaries.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=list(rows[0]),lineterminator='\n');writer.writeheader();writer.writerows(rows)
meta['allocation_runs']=[]
for label in binaries:
    print('Headless allocation validator removal',label,flush=True)
    command=[str(binaries[label]),'--samples','6','--warmup','2','--in-flight','1','--panes','16,128,1024'];item={'condition':label,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
    result=subprocess.run(command,text=True,capture_output=True);(output/f'allocations-{label}.csv').write_text(result.stdout);(output/f'allocations-{label}.stderr').write_text(result.stderr)
    item.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat());meta['allocation_runs'].append(item);(output/'environment.json').write_text(json.dumps(meta,indent=2)+'\n');assert result.returncode==0
meta.update(finished_utc=datetime.now(timezone.utc).isoformat(),results_sha256={name:hashlib.sha256((output/name).read_bytes()).hexdigest() for name in ['process-summaries.csv','allocations-baseline.csv','allocations-candidate.csv']});(output/'environment.json').write_text(json.dumps(meta,indent=2)+'\n')
assert all(hashlib.sha256((repo/n).read_bytes()).hexdigest()==h for n,h in state['production_sha256'].items())
print('All validator-removal qualification finished',flush=True)
