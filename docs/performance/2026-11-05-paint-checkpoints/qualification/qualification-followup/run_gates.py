import json, os, subprocess
from datetime import datetime, timezone
from pathlib import Path
out = Path(__file__).resolve().parent
gates = json.loads((out / 'gates.json').read_text())
results = []
for name, command in gates:
    started = datetime.now(timezone.utc).isoformat()
    print(f'START {name} {started}', flush=True)
    environment = dict(os.environ, CARGO_BUILD_JOBS='1')
    if name.startswith('docs-'):
        environment['RUSTDOCFLAGS'] = '-D warnings'
    with (out / f'{name}.log').open('w') as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, env=environment)
    entry = dict(gate=name, command=command, start_utc=started, end_utc=datetime.now(timezone.utc).isoformat(), returncode=result.returncode)
    results.append(entry)
    (out / 'validation-results.json').write_text(json.dumps(results,indent=2)+'\n')
    print(f'END {name} rc={result.returncode}', flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
