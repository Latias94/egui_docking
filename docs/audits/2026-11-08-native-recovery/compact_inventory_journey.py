import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-inventory-final')
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
        xdo('windowsize', host, 440, 620)
        time.sleep(1)
        capture('compact-editor.png')
        click(host, 307, 34)
        capture('compact-inventory.png')
        xdo('mousemove', '--window', host, 330, 131)
        time.sleep(1)
        capture('inspector-actions.png')
        click(host, 130, 361)
        capture('inspector-unloaded.png')
        click(host, 307, 34)
        xdo('mousemove', '--window', host, 330, 131)
        time.sleep(1)
        capture('inspector-reload-menu.png')
        click(host, 130, 361)
        capture('inspector-reloaded.png')
        click(host, 307, 34)
        xdo('mousemove', '--window', host, 330, 131)
        time.sleep(1)
        click(host, 130, 306)
        capture('inspector-close-request.png')
        click(host, 119, 207)
        capture('inspector-closed.png')
        click(host, 307, 34)
        xdo('mousemove', '--window', host, 330, 131)
        time.sleep(1)
        capture('inspector-reopen-menu.png')
        click(host, 130, 173)
        capture('inspector-reopened.png')
        assert find_window('Dockspace native editor') == host
        geometry = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', host).splitlines())
        assert geometry['WIDTH'] == '440'
        (root / 'window-state.json').write_text(json.dumps({'host': host, 'host_width': int(geometry['WIDTH']), 'active_window': xdo('getactivewindow'), 'app_returncode_before_cleanup': app.poll()}, indent=2) + '\n')
        assert app.poll() is None, 'the actual editor must remain running through the menu actions'
        print('Captured actual compact menu, content toggle, explicit close approval and reopening; screenshot state requires inspection.', flush=True)
    finally:
        if held:
            xdo('mouseup', 1)
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
