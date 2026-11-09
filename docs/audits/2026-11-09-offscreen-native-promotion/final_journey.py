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
        xdo('mousemove', '--window', host, 930, 350)
        xdo('click', 1)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 50, '~OFFSCREEN_RETAINED~')
        capture('inspector-before-shrink.png')
        xdo('windowsize', host, 680, 620)
        time.sleep(0.8)
        xdo('mousemove', '--window', host, 290, 11)
        xdo('click', 1)
        time.sleep(0.8)
        capture('native-open-result.png')
        identity = {
            'binary_sha256': hashlib.sha256(Path('/native-target/debug/examples/basic').read_bytes()).hexdigest(),
            'source_head': subprocess.check_output(['git', '-C', '/Users/frankorz/codes/rust/egui_docking', 'rev-parse', 'HEAD'], text=True).strip(),
            'host_window': host, 'host_geometry': xdo('getwindowgeometry', host),
            'active_window': xdo('getactivewindow'), 'app_alive': app.poll() is None,
        }
        (root / 'identity.json').write_text(json.dumps(identity, indent=2) + '\n')
        child = find_window('Dockspace panel')
        time.sleep(0.8)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 50, '~OFFSCREEN_PROMOTED~')
        capture('child-input.png')
        identity.update(child_window=child, active_window=xdo('getactivewindow'), app_alive=app.poll() is None)
        (root / 'identity.json').write_text(json.dumps(identity, indent=2) + '\n')
        assert identity['active_window'] == child
        assert find_window('Dockspace native editor') == host
        assert app.poll() is None
        xdo('key', 'alt+F4')
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            result = subprocess.run(
                ['xdotool', 'search', '--onlyvisible', '--name', '^Dockspace panel$'],
                capture_output=True, text=True, timeout=3,
            )
            if result.returncode != 0:
                break
            time.sleep(0.2)
        else:
            raise RuntimeError('Recovered child remained visible')
        xdo('windowactivate', '--sync', host)
        time.sleep(0.6)
        capture('recovered-narrow-host.png')
        xdo('windowsize', host, 1280, 780)
        time.sleep(0.7)
        capture('recovered-content.png')
        identity.update(active_window=xdo('getactivewindow'), child_retired=True, app_alive=app.poll() is None)
        (root / 'identity.json').write_text(json.dumps(identity, indent=2) + '\n')
        assert identity['active_window'] == host
        assert app.poll() is None
        print('Exact child window appeared from a fully offscreen group; inspect retained and new input text in screenshots.', flush=True)
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
