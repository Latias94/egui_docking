import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-overflow-final')
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
        click(host, 70, 270)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~OUTLINE_BEFORE~')
        capture('outline-edited.png')
        click(host, 122, 166)
        xdo('mousemove', 20, 20)
        capture('overflow-open.png')
        xdo('key', 'End')
        time.sleep(0.4)
        capture('overflow-end.png')
        xdo('key', 'Return')
        time.sleep(0.8)
        capture('overflow-enter.png')
        xdo('type', '--clearmodifiers', '--delay', 20, '~ASSETS_AFTER~')
        capture('assets-edit.png')
        click(host, 122, 166)
        xdo('mousemove', 20, 20)
        xdo('key', 'End')
        time.sleep(0.3)
        xdo('key', 'space')
        time.sleep(0.8)
        xdo('type', '--clearmodifiers', '--delay', 20, '~ASSETS_REPEAT~')
        capture('assets-repeat-edit.png')
        click(host, 122, 166)
        xdo('mousemove', 20, 20)
        capture('second-open.png')
        xdo('key', 'Home')
        time.sleep(0.3)
        xdo('key', 'Return')
        time.sleep(0.8)
        capture('overflow-home-enter.png')
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~OUTLINE_AFTER~')
        capture('outline-after-edit.png')
        assert find_window('Dockspace native editor') == host
        assert xdo('getactivewindow') == host
        (root / 'window-state.json').write_text(json.dumps({'host': host, 'active_window': xdo('getactivewindow'), 'app_returncode_before_cleanup': app.poll()}, indent=2) + '\n')
        assert app.poll() is None
        print('Captured actual overflow-menu End/Enter, repeated End/Space and Home/Enter with direct edits; inspect screenshot state.', flush=True)
    finally:
        if held:
            xdo('mouseup', 1)
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
