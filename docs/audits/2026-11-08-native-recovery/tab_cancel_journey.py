import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-restore-cancel-ui')
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
        click(host, 248, 11)
        child = find_window('Dockspace panel')
        time.sleep(1)
        xdo('windowactivate', '--sync', host)
        click(host, 307, 34)
        click(host, 403, 130)
        assert xdo('getactivewindow') == child
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~CANCEL_BEFORE~')
        capture('cancel-before.png')
        xdo('mousemove', '--window', child, 160, 14)
        xdo('mousedown', 1)
        held = True
        time.sleep(0.2)
        start = dict(line.split('=',1) for line in xdo('getmouselocation','--shell').splitlines())
        geometry = dict(line.split('=',1) for line in xdo('getwindowgeometry', '--shell', host).splitlines())
        sx, sy = int(start['X']), int(start['Y'])
        tx, ty = int(geometry['X']) + 700, int(geometry['Y']) + 350
        for step in range(1, 13):
            xdo('mousemove', round(sx + (tx-sx)*step/12), round(sy + (ty-sy)*step/12))
            time.sleep(0.035)
        time.sleep(0.3)
        capture('cancel-dragging.png')
        xdo('key', 'Escape')
        time.sleep(0.3)
        xdo('mouseup', 1)
        held = False
        time.sleep(0.8)
        capture('cancel-after.png')
        active_after_cancel = xdo('getactivewindow')
        xdo('type', '--clearmodifiers', '--delay', 20, '~CANCEL_AFTER~')
        capture('cancel-edit.png')
        assert find_window('Dockspace panel') == child
        xdo('windowactivate', '--sync', host)
        click(host, 260, 57)
        capture('preset-saved.png')
        click(host, 190, 34)
        time.sleep(1)
        capture('default-layout.png')
        click(host, 380, 57)
        time.sleep(2)
        fresh = find_window('Dockspace panel')
        assert fresh != child, 'Restore must create a fresh native child'
        capture('preset-restored.png')
        xdo('windowactivate', '--sync', host)
        click(host, 307, 34)
        click(host, 403, 130)
        time.sleep(1)
        assert xdo('getactivewindow') == fresh
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~RESTORED_EDIT~')
        capture('restored-edit.png')
        (root / 'journey.json').write_text(json.dumps({'host': host, 'child': child, 'fresh': fresh, 'active_after_cancel': active_after_cancel, 'returncode': 0}, indent=2) + '\n')
        print('Captured actual native drag cancellation, editing, toolbar save/default/restore and editing a fresh child.', flush=True)
    finally:
        if held:
            xdo('mouseup', 1)
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
