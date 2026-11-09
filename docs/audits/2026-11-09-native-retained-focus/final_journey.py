import hashlib
import json
from pathlib import Path
import subprocess
import time
import sys

root = Path(sys.argv[1])
root.mkdir(exist_ok=True)


def xdo(*arguments):
    return subprocess.check_output(
        ['xdotool', *map(str, arguments)], text=True, timeout=5
    ).strip()


def capture(name):
    subprocess.run(['xrefresh'], check=True, timeout=5)
    time.sleep(0.25)
    subprocess.run(
        ['import', '-window', 'root', str(root / name)], check=True, timeout=10
    )


def find_window(title):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        result = subprocess.run(
            ['xdotool', 'search', '--onlyvisible', '--name', '^' + title + '$'],
            capture_output=True, text=True, timeout=3,
        )
        if result.returncode == 0:
            return result.stdout.splitlines()[-1]
        time.sleep(0.2)
    raise RuntimeError('Window did not appear: ' + title)


manager = subprocess.Popen(
    ['openbox'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
)
with (root / 'journey.stderr').open('w') as errors:
    app = subprocess.Popen(
        ['/native-target/debug/examples/basic'],
        stdout=subprocess.DEVNULL, stderr=errors,
    )
    try:
        host = find_window('Dockspace native editor')
        xdo('windowactivate', '--sync', host)
        time.sleep(2)
        xdo('windowsize', host, 1000, 780)
        xdo('windowmove', host, 40, 80)
        time.sleep(1)
        xdo('mousemove', '--window', host, 290, 11)
        xdo('click', 1)
        child = find_window('Dockspace panel')
        xdo('windowmove', child, 940, 120)
        time.sleep(0.8)
        observations = []
        def record(step, target):
            observations.append({'step': step, 'active': xdo('getactivewindow'), 'target': target,
                                 'app_alive': app.poll() is None})
            (root / 'observations.json').write_text(json.dumps(observations, indent=2) + '\n')
            assert observations[-1]['active'] == target, observations[-1]
            assert app.poll() is None
        for label, window, point in [('host', host, (400, 350)), ('child', child, (220, 100))]:
            xdo('windowactivate', '--sync', window)
            xdo('mousemove', '--window', window, *point)
            xdo('click', 1)
            time.sleep(0.3)
            xdo('key', 'ctrl+End')
            xdo('type', '--clearmodifiers', '--delay', 50, '~' + label.upper() + '_BEFORE~')
            capture(label + '-before.png')
        xdo('mousemove', 20, 20)
        for cycle in range(6):
            for label, window in [('host', host), ('child', child)]:
                xdo('windowactivate', '--sync', window)
                time.sleep(0.3)
                xdo('windowminimize', window)
                time.sleep(0.6)
                state = subprocess.check_output(['xprop', '-id', window, '_NET_WM_STATE'], text=True)
                assert '_NET_WM_STATE_HIDDEN' in state, state
                xdo('windowactivate', '--sync', window)
                time.sleep(0.8)
                xdo('type', '--clearmodifiers', '--delay', 50, f'~{label.upper()}_RESTORED_{cycle}~')
                capture(f'{label}-restored-{cycle}.png')
                record(f'{label}-restored-{cycle}', window)
        xdo('windowactivate', '--sync', host)
        xdo('key', 'ctrl+equal')
        time.sleep(0.3)
        xdo('key', 'ctrl+equal')
        time.sleep(0.8)
        xdo('mousemove', '--window', host, 400, 350)
        xdo('click', 1)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 50, '~HOST_ZOOM~')
        capture('zoom-host-input.png')
        record('zoom-host-input', host)
        xdo('windowactivate', '--sync', child)
        xdo('mousemove', '--window', child, 200, 180)
        xdo('click', 1)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 50, '~CHILD_ZOOM~')
        capture('zoom-child-input.png')
        record('zoom-child-input', child)
        xdo('windowactivate', '--sync', host)
        xdo('mousemove', '--window', host, 184, 204)
        xdo('click', 1)
        time.sleep(0.6)
        capture('zoom-tab-menu.png')
        xdo('mousemove', '--window', host, 164, 278)
        xdo('click', 1)
        time.sleep(0.6)
        capture('zoom-menu-selected-assets.png')
        (root / 'identity.json').write_text(json.dumps({
            'binary_sha256': hashlib.sha256(Path('/native-target/debug/examples/basic').read_bytes()).hexdigest(),
            'source_head': subprocess.check_output(['git', '-C', '/Users/frankorz/codes/rust/egui_docking', 'rev-parse', 'HEAD'], text=True).strip(),
            'host': host, 'child': child, 'app_alive': app.poll() is None,
        }, indent=2) + '\n')
        assert app.poll() is None
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
