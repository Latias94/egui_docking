import argparse, importlib.util, json, subprocess
from pathlib import Path
repo = Path('/Users/frankorz/codes/rust/egui_docking')
root = Path(__file__).resolve().parent
parser = argparse.ArgumentParser()
parser.add_argument('condition')
parser.add_argument('output')
args = parser.parse_args()
out = root / args.output
out.mkdir(exist_ok=False)
spec = importlib.util.spec_from_file_location('comparison', repo / 'scripts/compare_products.py')
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)
binaries = {'egui_dockspace': root / (args.condition + '-egui_dockspace'), 'dear_imgui': root / 'qualified-dear_imgui'}
metadata = {'condition': args.condition, 'build_profile': {'rust_lto':'thin','rust_codegen_units':1,'cpp_flags':['-O3','-DNDEBUG','-flto=thin'],'commands':'../candidate-seventeen-commands.json', 'cpp_build':'../qualified-one/environment.json', 'delivery_profile':'production (release + thin LTO + one codegen unit)' }, 'scope': 'Frozen executables; timing only; no builds/tests/sampling overlap. Complete unchanged product workload.', 'samples':24, 'warmup':6, 'panes':[16,128,1024], 'started_utc':comparison.utc_now(), 'binary_sha256':{p:comparison.digest(b) for p,b in binaries.items()}, 'runs':[]}
rows = []
for run, product in enumerate(['egui_dockspace','dear_imgui','dear_imgui','egui_dockspace'],1):
    binary = binaries[product]
    assert comparison.digest(binary) == metadata['binary_sha256'][product]
    command = [str(binary),'--samples','24','--warmup','6','--panes','16,128,1024']
    started = comparison.utc_now()
    print('Run',run,args.condition,product,flush=True)
    result = subprocess.run(command,cwd=repo,text=True,capture_output=True)
    (out / f'run-{run}-{product}.csv').write_text(result.stdout)
    (out / f'run-{run}-{product}.stderr').write_text(result.stderr)
    metadata['runs'].append({'run':run,'product':product,'command':command,'started_utc':started,'finished_utc':comparison.utc_now(),'returncode':result.returncode})
    (out/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
    if result.returncode: raise RuntimeError(result.stderr[-6000:])
    rows.extend({'run':run,**row} for row in comparison.validate_rows(result.stdout,product,[16,128,1024],24))
comparison.write_csv(out/'raw.csv',['run',*comparison.HEADER],rows)
summaries=[]
for product in comparison.PRODUCTS:
    for n in [16,128,1024]:
        for scenario,frames in comparison.SCENARIOS.items():
            selected=[row for row in rows if row['product']==product and row['panes']==n and row['scenario']==scenario]
            values=[row['elapsed_ns'] for row in selected]
            summary={'product':product,'panes':n,'scenario':scenario,'frames_per_sample':frames,'samples':len(values),'p50_ns':comparison.percentile(values,50),'p95_ns':comparison.percentile(values,95),'max_ns':max(values)}
            for key in ['passes','content_calls','visible_contents','vertices','indices','draw_commands']:
                summary['p50_'+key]=comparison.percentile([row[key] for row in selected],50)
            summaries.append(summary)
comparison.write_csv(out/'summary.csv',list(summaries[0]),summaries)
metadata['finished_utc']=comparison.utc_now()
metadata['results_sha256']={name:comparison.digest(out/name) for name in ['raw.csv','summary.csv']}
(out/'environment.json').write_text(json.dumps(metadata,indent=2)+'\n')
for row in summaries:
    if row['panes']==1024:print(row['product'],row['scenario'],round(row['p50_ns']/1e6,3),flush=True)
print('Complete',len(rows),'rows',flush=True)
