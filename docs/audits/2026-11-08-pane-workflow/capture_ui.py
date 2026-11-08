import json
import os
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-pane-workflow-ui')
root.mkdir(exist_ok=True)
manager = subprocess.Popen(['openbox'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
actions = []
with (root / 'actual-ui.stderr').open('w') as errors:
    app = subprocess.Popen(['/native-target/debug/examples/basic'], stdout=subprocess.DEVNULL, stderr=errors)
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            found = subprocess.run(['xdotool', 'search', '--name', '^Dockspace native editor$'], capture_output=True, text=True)
            if found.returncode == 0:
                window = found.stdout.splitlines()[-1]
                break
            time.sleep(0.2)
        else:
            raise RuntimeError('The native editor window did not appear')
        subprocess.run(['xdotool', 'windowactivate', '--sync', window], check=True, timeout=5)
        time.sleep(2)
        subprocess.run(['import', '-window', 'root', str(root / 'before.png')], check=True, timeout=10)
        print('Ready: inspect before.png, then send window-relative x y for inventory, Move and Float.', flush=True)
        for name in ['inventory.png', 'move-menu.png', 'after-float.png']:
            x, y = map(int, input().split())
            subprocess.run(['xdotool', 'mousemove', '--window', window, str(x), str(y)], check=True, timeout=3)
            subprocess.run(['xdotool', 'click', '1'], check=True, timeout=3)
            time.sleep(1)
            subprocess.run(['import', '-window', 'root', str(root / name)], check=True, timeout=10)
            actions.append({'image': name, 'click': [x, y]})
            print('Captured ' + name, flush=True)
        (root / 'ui-actions.json').write_text(json.dumps({'display': os.environ['DISPLAY'], 'window': window, 'actions': actions}, indent=2) + '\n')
        assert app.poll() is None, 'The editor exited during the pane operation'
        print('Actual native editor remained live after inventory Float.', flush=True)
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
