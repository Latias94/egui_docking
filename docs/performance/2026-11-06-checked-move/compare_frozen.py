"""One-off baseline/candidate comparison using the unchanged docking protocol."""
import csv, hashlib, json, os, shutil, subprocess, sys
from collections import defaultdict
from pathlib import Path

ROOT = Path('/Users/frankorz/codes/rust/egui_docking')
sys.path.insert(0, str(ROOT / 'scripts'))
from compare_docking import HEADER, PHASES, CASES, validate
from compare_products import percentile, utc_now, write_csv

BASE = Path('/tmp/dockspace-checked-move-20261106-a2bcabc')
OUTPUT = BASE / 'frozen-comparison-v2'
OUTPUT.mkdir()
binaries = {'baseline': BASE / 'baseline-docking-cost',
            'candidate': BASE / 'candidate-reference-v2' / 'dockspace_docking_cost'}
source_receipt = json.loads((BASE / 'candidate-reference-v2/environment.json').read_text())
hash_file = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
metadata = {'started_utc': utc_now(), 'baseline_commit': 'a2bcabc1440caa664fb5f4a64c4669c4eecfdce4',
            'scope': source_receipt['scope'], 'limits': source_receipt['limits'],
            'candidate_source_receipt': str(BASE / 'candidate-reference-v2/environment.json'),
            'candidate_sources': source_receipt['sources'], 'samples': 48, 'warmup': 8,
            'panes': [16,128,1024], 'views': [1,4],
            'binaries': {name:{'path':str(path),'sha256':hash_file(path)} for name,path in binaries.items()},
            'runs': []}
rows=[]
dumps=defaultdict(set)
for group,order in [(1,('baseline','candidate','candidate','baseline')),
                    (2,('candidate','baseline','baseline','candidate'))]:
    for run,condition in enumerate(order,1):
        prefix=OUTPUT/f'group-{group}-run-{run}-{condition}'
        raw=prefix.with_suffix('.csv')
        state_dir=OUTPUT/(prefix.name+'-states')
        state_dir.mkdir()
        env=dict(os.environ,DOCKSPACE_DOCK_COST_SAMPLES='48',DOCKSPACE_DOCK_COST_WARMUP='8',
                 DOCKSPACE_DOCK_COST_PANES='16,128,1024',DOCKSPACE_DOCK_COST_VIEWS='1,4',
                 DOCKSPACE_DOCK_COST_OUT=str(raw),DOCKSPACE_DOCK_COST_STATE_DIR=str(state_dir))
        env.pop('DOCKSPACE_DOCK_COST_CASE',None)
        command=[str(binaries[condition]),'--exact','docking_cost::docking_cost','--ignored','--nocapture','--test-threads','1']
        receipt={'group':group,'run':run,'condition':condition,'command':command,'started_utc':utc_now()}
        print(group,run,condition,flush=True)
        result=subprocess.run(command,cwd=ROOT,env=env,text=True,capture_output=True)
        prefix.with_suffix('.stdout').write_text(result.stdout)
        prefix.with_suffix('.stderr').write_text(result.stderr)
        receipt.update(returncode=result.returncode,finished_utc=utc_now())
        metadata['runs'].append(receipt)
        (OUTPUT/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
        if result.returncode: raise RuntimeError(str(prefix))
        rows.extend(dict(group=group,run=run,condition=condition,**r) for r in validate(raw.read_text(),'dockspace',[16,128,1024],[1,4],48,8,None))
        expected={f'dockspace-{n}-{v}-{case}.txt' for n in (16,128,1024) for v in (1,4) for case in CASES if case!='cross_view_move' or v==4}
        assert {p.name for p in state_dir.iterdir()}==expected
        for p in state_dir.iterdir(): dumps[p.name].add(p.read_bytes())
assert all(len(values)==1 for values in dumps.values())
states=defaultdict(set)
for row in rows: states[row['panes'],row['views'],row['case'],row['width']].add(row['state_digest'])
assert all(len(values)==1 for values in states.values())
summary=[]
for group in (1,2):
 for n in (16,128,1024):
  for v in (1,4):
   for case in CASES:
    if case=='cross_view_move' and v==1: continue
    pair={}
    for condition in binaries:
     cell=[r for r in rows if (r['group'],r['panes'],r['views'],r['case'],r['condition'])==(group,n,v,case,condition)]
     entry=dict(group=group,condition=condition,panes=n,views=v,case=case,samples=len(cell))
     for phase in PHASES:
      values=[r[phase] for r in cell]
      entry.update({phase+'_p50':percentile(values,50),phase+'_p95':percentile(values,95),phase+'_max':max(values)})
     pair[condition]=entry
     summary.append(entry)
    if n==1024:
     old=pair['baseline']['operation_owned_ns_p50']; new=pair['candidate']['operation_owned_ns_p50']
     print(group,n,v,case,old,new,round((1-new/old)*100,3),flush=True)
write_csv(OUTPUT/'raw.csv',['group','run','condition',*HEADER],rows)
write_csv(OUTPUT/'summary.csv',list(summary[0]),summary)
assert all(hash_file(ROOT/name)==value for name,value in metadata['candidate_sources'].items())
assert all(hash_file(path)==metadata['binaries'][name]['sha256'] for name,path in binaries.items())
metadata.update(finished_utc=utc_now(),complete_state_equal=True,input_hashes_unchanged=True,
                result_sha256={name:hash_file(OUTPUT/name) for name in ('raw.csv','summary.csv')})
(OUTPUT/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
print('Frozen comparison complete:',OUTPUT)
