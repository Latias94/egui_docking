"""Sample frozen final product cases separately from formal timing and builds."""
from pathlib import Path
import subprocess,json,hashlib
from datetime import datetime,timezone
root=Path(__file__).resolve().parent;binary=root/'candidate-product-bin'
for scenario,samples in [('idle',1600),('tab_click',420)]:
 prefix=root/f'candidate-{scenario}'
 command=[str(binary),'--samples',str(samples),'--warmup','6','--panes','1024','--scenario',scenario]
 meta={'command':command,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'started_utc':datetime.now(timezone.utc).isoformat(),'scope':'Separate sampled CPU attribution including initialization, warmup and disposal; not formal timing.'}
 with prefix.with_suffix('.csv').open('w') as out,prefix.with_suffix('.stderr').open('w') as err:
  p=subprocess.Popen(command,stdout=out,stderr=err)
  meta['sample_command']=['sample',str(p.pid),'8','-file',str(prefix.with_suffix('.sample.txt'))]
  with prefix.with_suffix('.sampler.log').open('w') as log:
   meta['sampler_returncode']=subprocess.run(meta['sample_command'],stdout=log,stderr=subprocess.STDOUT).returncode
  meta['returncode']=p.wait()
 meta['finished_utc']=datetime.now(timezone.utc).isoformat()
 prefix.with_suffix('.json').write_text(json.dumps(meta,indent=2)+'\n')
 print(scenario,meta['returncode'],meta['sampler_returncode'],flush=True)
 assert meta['returncode']==meta['sampler_returncode']==0
for label in ['baseline','candidate']:
 for case in ['idle','tab_click']:
  prefix=root/f'{label}-{case}'
  with (root/f'{label}-{case}-attribution.json').open('w') as out:
   subprocess.run(['python3',str(root/'summarize_cpu.py'),str(prefix.with_suffix('.sample.txt'))],stdout=out,check=True)
print('Separate profiles complete',flush=True)
