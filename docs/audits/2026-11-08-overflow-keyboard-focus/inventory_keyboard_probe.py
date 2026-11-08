import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-keyboard-audit')
root.mkdir(exist_ok=True)

def xdo(*arguments):
    return subprocess.check_output(['xdotool', *map(str, arguments)], text=True, timeout=5).strip()

def click(window, x, y):
    xdo('mousemove', '--window', window, x, y)
    xdo('click', 1)
    time.sleep(0.7)

def capture(name):
    subprocess.run(['xrefresh'], check=True, timeout=5)
    time.sleep(0.25)
    subprocess.run(['import', '-window', 'root', str(root / name)], check=True, timeout=10)

def find_window(title):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^' + title + '$'], capture_output=True, text=True, timeout=3)
        if result.returncode == 0:
            return result.stdout.splitlines()[-1]
        time.sleep(0.2)
    raise RuntimeError('Window did not appear: ' + title)

manager = subprocess.Popen(['openbox'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
with (root / 'journey.stderr').open('w') as errors:
    app = subprocess.Popen(['/native-target/debug/examples/basic'], stdout=subprocess.DEVNULL, stderr=errors)
    held = False
    try:
        host = find_window('Dockspace native editor')
        xdo('windowactivate', '--sync', host)
        time.sleep(2)
        xdo('windowsize', host, 680, 620)
        time.sleep(1)
        xdo('mousemove', 20, 20)
        xdo('key', 'Escape')
        for step in range(1, 6):
            xdo('key', 'Tab')
            time.sleep(0.3)
            capture(f'tab-{step}.png')
        xdo('key', 'shift+Tab')
        time.sleep(0.3)
        capture('inventory-focused.png')
        xdo('key', 'Return')
        time.sleep(0.4)
        capture('inventory-enter.png')
        for step in range(1, 5):
            xdo('key', 'Down')
            time.sleep(0.25)
            capture(f'inventory-down-{step}.png')
        xdo('key', 'Right')
        time.sleep(0.4)
        capture('inventory-right.png')
        xdo('key', 'Return')
        time.sleep(0.8)
        capture('inventory-action.png')
        assert app.poll() is None
        print('Captured keyboard focus traversal and pane inventory navigation; images require inspection.', flush=True)
    finally:
        if held:
            xdo('mouseup', 1)
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
