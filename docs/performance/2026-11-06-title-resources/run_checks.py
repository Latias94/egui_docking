import hashlib,json,os,subprocess
from datetime import datetime,timezone
from pathlib import Path
root=Path(__file__).resolve().parent
state=json.loads((root/'state.json').read_text())
source=state['production_sha256']
def verify():
 for p,h in {**source,**state['user_preservation']}.items():
  assert hashlib.sha256(Path(p).read_bytes()).hexdigest()==h,p
verify()
commands=json.loads((root/'gate-commands.json').read_text())
assert len(commands)==12
results=json.loads((root/'checks.json').read_text()) if (root/'checks.json').exists() else []
assert all(x['returncode']==0 for x in results)
for item in commands[len(results):]:
 name=item['check'];command=item['command']
 print('START '+name,flush=True)
 receipt={'check':name,'command':command,'started_utc':datetime.now(timezone.utc).isoformat(),'source_snapshot_sha256':hashlib.sha256(json.dumps(source,sort_keys=True).encode()).hexdigest()}
 with (root/(name+'.log')).open('w') as log:
  run=subprocess.run(command,env=dict(os.environ,CARGO_BUILD_JOBS='1'),stdout=log,stderr=subprocess.STDOUT)
 receipt.update(returncode=run.returncode,finished_utc=datetime.now(timezone.utc).isoformat())
 results.append(receipt)
 (root/'checks.json').write_text(json.dumps(results,indent=2)+'\n')
 print('FINISH '+name+' exit '+str(run.returncode),flush=True)
 if run.returncode: raise SystemExit(run.returncode)
verify()
print('Final gates passed on exact source snapshot',flush=True)
