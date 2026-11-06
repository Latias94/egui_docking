"""Run the unchanged layout/product probes and freeze their actual artifacts."""

from datetime import datetime, timezone
from pathlib import Path
import hashlib
import json
import shutil
import subprocess
import sys

base = Path(sys.argv[1])
post = json.loads((base/'post-review-checks.json').read_text())
assert len(post)==4 and all(item['returncode']==0 for item in post)
commands = [
    ('candidate-layout', ['python3','scripts/compare_layouts.py','--out',str(base/'candidate-layout')]),
    ('candidate-product', ['python3','scripts/compare_products.py','--backend','fork','--profile','production',
                           '--samples','24','--warmup','6','--panes','16,128,1024','--runs','4',
                           '--out',str(base/'candidate-product')]),
    ('frozen-comparison', ['python3',str(base/'run_frozen_comparison.py'),str(base)]),
    ('verification', ['python3',str(base/'verify_results.py'),str(base)]),
]
rows = []
for name, command in commands:
    print('Running',name,flush=True)
    row = {'name':name,'command':command,'started_utc':datetime.now(timezone.utc).isoformat()}
    with (base/(name+'.log')).open('w') as log:
        result = subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    row.update(returncode=result.returncode,finished_utc=datetime.now(timezone.utc).isoformat())
    rows.append(row)
    (base/'candidate-commands.json').write_text(json.dumps(rows,indent=2)+'\n')
    if result.returncode:
        print((base/(name+'.log')).read_text()[-6000:]);raise SystemExit(result.returncode)
    if name in ['candidate-layout','candidate-product']:
        metadata=json.loads((base/name/'environment.json').read_text())
        if name=='candidate-product':
            artifacts={key:Path(path) for key,path in metadata['binary_paths'].items()}
        else:
            artifacts={'dear_imgui':base/name/'imgui_layout_cost'}
            for line in (base/name/'build-rust.log').read_text().splitlines():
                try:
                    event=json.loads(line)
                except json.JSONDecodeError:
                    continue
                if event.get('reason')=='compiler-artifact' and event.get('target',{}).get('name')=='dockspace' and event.get('profile',{}).get('test') and event.get('executable'):
                    artifacts['dockspace']=Path(event['executable'])
        for product,path in artifacts.items():
            destination=base/f'{name}-{product}'
            shutil.copy2(path,destination)
            assert hashlib.sha256(destination.read_bytes()).hexdigest()==metadata['binary_sha256'][product]
        print('Frozen artifacts for',name,flush=True)
print('All comparisons and independent verification complete',flush=True)
