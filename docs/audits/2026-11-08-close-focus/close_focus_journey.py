import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-close-journey')
root.mkdir(exist_ok=True)

def xdo(*arguments):
    return subprocess.check_output(['xdotool', *map(str, arguments)], text=True, timeout=5).strip()

def click(window, x, y):
    xdo('mousemove', '--window', window, x, y)
    xdo('click', 1)
    time.sleep(0.6)

def capture(name):
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
    try:
        host = find_window('Dockspace native editor')
        xdo('windowactivate', '--sync', host)
        time.sleep(2)
        click(host, 248, 11)
        child = find_window('Dockspace panel')
        time.sleep(1)
        xdo('windowactivate', '--sync', host)
        click(host, 307, 34)
        capture('inventory-probe.png')
        click(host, 403, 130)
        time.sleep(1)
        capture('after-select.png')
        active = xdo('getactivewindow')
        assert active == child, f'Inventory selection did not activate its child: {active} != {child}'
        xdo('type', '--clearmodifiers', '--delay', 40, '~FOCUS_RECOVERY~')
        time.sleep(0.5)
        capture('focused-child.png')
        xdo('windowactivate', '--sync', host)
        time.sleep(1)
        click(host, 307, 34)
        click(host, 550, 130)
        time.sleep(0.6)
        capture('close-before.png')
        click(host, 280, 191)
        capture('close-postponed.png')
        click(host, 111, 167)
        capture('close-partly-confirmed.png')
        click(host, 320, 191)
        capture('close-cancelled.png')
        assert find_window('Dockspace panel') == child, 'Keeping Inspector open must preserve the complete native group'
        click(host, 307, 34)
        click(host, 550, 130)
        capture('close-second-request.png')
        click(host, 111, 167)
        click(host, 121, 191)
        time.sleep(1)
        capture('group-closed.png')
        child_search = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^Dockspace panel$'], capture_output=True, text=True, timeout=3)
        assert child_search.returncode == 1, 'Approving every member must close the native group'
        click(host, 307, 34)
        capture('inventory-closed.png')
        click(host, 403, 130)
        time.sleep(1)
        capture('inspector-reopened.png')
        (root / 'journey.json').write_text(json.dumps({'host': host, 'child': child, 'active_after_select': active, 'marker': '~FOCUS_RECOVERY~', 'retained_group_after_keep_open': child, 'child_search_after_close': child_search.returncode, 'returncode': 0}, indent=2) + '\n')
        print('Selected the existing OS child, typed the marker, postponed and cancelled a group close, approved every member, observed the child disappear, and reopened Inspector.', flush=True)
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
