"""Build and measure isolated DHAT versions, separately from product CPU timing."""
from pathlib import Path
from datetime import datetime,timezone
import csv,hashlib,io,json,os,shutil,subprocess
root=Path(__file__).resolve().parent; repo=Path.cwd();receipts=[]
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
now=lambda:datetime.now(timezone.utc).isoformat()
state=json.loads((root/'state.json').read_text())
assert all(digest(repo/p)==h for p,h in state['production_sha256'].items())
assert json.loads((root/'code-review-exit.json').read_text())['returncode']==0
for label in ['baseline','candidate']:
 folder=root/f'allocation-{label}'
 for step,command in [('lock',['cargo','generate-lockfile','--manifest-path',str(folder/'Cargo.toml'),'--offline']),('build',['cargo','build','--manifest-path',str(folder/'Cargo.toml'),'--profile','production','--locked','--offline','--target-dir',str(repo/'integration/egui-fork-workspace/target'),'--message-format=json-render-diagnostics'])]:
  print(label,step,flush=True);r={'condition':label,'step':step,'command':command,'started_utc':now()}
  with (root/f'allocation-{label}-{step}.log').open('w') as log:
   result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,env=dict(os.environ,CARGO_BUILD_JOBS='1'))
  r.update(returncode=result.returncode,finished_utc=now());receipts.append(r)
  (root/'allocation-commands.json').write_text(json.dumps(receipts,indent=2)+'\n')
  assert result.returncode==0,(label,step)
 binary=repo/'integration/egui-fork-workspace/target/production/dockspace-product-allocation-probe'
 shutil.copy2(binary,root/f'allocation-{label}-bin')
assert (root/'allocation-baseline/Cargo.lock').read_bytes() == (root/'allocation-candidate/Cargo.lock').read_bytes()
for run,label in enumerate(['baseline','candidate','candidate','baseline'],1):
 binary=root/f'allocation-{label}-bin';command=[str(binary),'--samples','6','--warmup','2','--panes','16,128,1024']
 print('Allocation run',run,label,flush=True)
 r={'run':run,'condition':label,'command':command,'binary_sha256':digest(binary),'started_utc':now()}
 with (root/f'allocation-run-{run}-{label}.csv').open('w') as out,(root/f'allocation-run-{run}-{label}.stderr').open('w') as err:
  result=subprocess.run(command,stdout=out,stderr=err)
 r.update(returncode=result.returncode,finished_utc=now());receipts.append(r)
 (root/'allocation-commands.json').write_text(json.dumps(receipts,indent=2)+'\n')
 assert result.returncode==0
assert all(digest(repo/p)==h for p,h in state['production_sha256'].items())
print('Separate DHAT runs complete',flush=True)
