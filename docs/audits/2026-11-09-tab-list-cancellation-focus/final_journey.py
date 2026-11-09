import hashlib
import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-menu-cancel-qualified')
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
        time.sleep(0.3)
        xdo('key', 'Escape')
        time.sleep(0.7)

        capture('after-escape-input.png')
        xdo('key', 'Return')
        time.sleep(0.3)
        capture('after-escape-enter.png')
        xdo('key', 'End')
        time.sleep(0.3)
        xdo('key', 'Return')
        time.sleep(0.7)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~AFTER_CANCEL_REOPEN~')
        capture('after-reopen-select-type.png')
        assert xdo('getactivewindow') == host
        xdo('windowsize', host, 1000, 780)
        time.sleep(0.8)
        click(host, 290, 11)
        capture('open-child-attempt.png')
        child = find_window('Dockspace panel')
        xdo('windowsize', child, 160, 260)
        xdo('windowmove', child, 940, 120)
        time.sleep(0.7)
        capture('native-small-tabs.png')
        click(child, 118, 14)
        xdo('mousemove', 20, 20)
        xdo('key', 'Home')
        time.sleep(0.3)
        xdo('key', 'Escape')
        time.sleep(0.7)
        capture('child-after-escape.png')
        xdo('key', 'Return')
        time.sleep(0.5)
        capture('child-after-reopen.png')
        xdo('key', 'End')
        time.sleep(0.3)
        xdo('key', 'Return')
        time.sleep(0.7)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~CHILD_CANCEL_REOPEN~')
        capture('child-after-reopen-select-type.png')
        assert xdo('getactivewindow') == child
        assert find_window('Dockspace native editor') == host
        (root / 'identity.json').write_text(json.dumps({
            'binary_sha256': hashlib.sha256(Path('/native-target/debug/examples/basic').read_bytes()).hexdigest(),
            'source_head': subprocess.check_output(['git', '-C', '/Users/frankorz/codes/rust/egui_docking', 'rev-parse', 'HEAD'], text=True).strip(),
            'active_window': xdo('getactivewindow'), 'host_window': host, 'child_window': child,
            'app_alive': app.poll() is None,
            'observed_text_and_menu_verdict': 'inspect original screenshots separately from process/window assertions',
        }, indent=2) + '\n')
        print('Main and child Escape/Enter journeys completed; inspect menu and text screenshots.', flush=True)
        assert app.poll() is None
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
