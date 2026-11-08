import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-keyboard-final')
root.mkdir(exist_ok=True)

def xdo(*arguments):
    return subprocess.check_output(['xdotool', *map(str, arguments)], text=True, timeout=5).strip()

def click(window, x, y):
    xdo('mousemove', '--window', window, x, y)
    xdo('click', 1)
    time.sleep(0.6)

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
with (root / 'focus-probe.stderr').open('w') as errors:
    app = subprocess.Popen(['/native-target/debug/examples/basic'], stdout=subprocess.DEVNULL, stderr=errors)
    try:
        host = find_window('Dockspace native editor')
        xdo('windowactivate', '--sync', host)
        time.sleep(2)
        click(host, 330, 160)
        time.sleep(1)
        capture('content-focus.png')
        xdo('mousemove', '--window', host, 700, 600)
        for _ in range(7):
            xdo('key', 'shift+Tab')
            time.sleep(0.3)
        capture('inspector-tab-focus.png')
        for key, name in [
            ('Home', 'home-preview'),
            ('End', 'end-inspector'),
            ('Home', 'second-home-preview'),
            ('Right', 'right-inspector'),
            ('Left', 'left-preview'),
            ('End', 'final-end-inspector'),
        ]:
            xdo('key', key)
            time.sleep(0.8)
            capture(name + '.png')
        xdo('key', 'Return')
        time.sleep(0.8)
        xdo('type', '--clearmodifiers', '--delay', 40, '~KEYBOARD_ACTIVATION~')
        capture('enter-activates-content.png')
        (root / 'keyboard.json').write_text(json.dumps({
            'host': host,
            'navigation': ['Home', 'End', 'Home', 'Right', 'Left', 'End'],
            'activation': 'Return',
            'marker': '~KEYBOARD_ACTIVATION~',
            'verification': 'Inspect each destination and its focus ring in the captured screenshots; exit alone does not assert widget state.',
        }, indent=2) + '\n')
        print('Captured repeated tab navigation and Enter content activation for visual verification.', flush=True)
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
