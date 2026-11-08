import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-window-focus-probe')
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
        xdo('mousemove', '--window', host, 400, 350)
        xdo('click', 1)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 25, '~ROOT_FIRST~')
        capture('root-edited.png')
        xdo('mousemove', '--window', host, 290, 11)
        xdo('click', 1)
        child = find_window('Dockspace panel')
        time.sleep(1)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 25, '~CHILD_CREATED~')
        capture('child-created.png')
        observations = []
        for label, window, marker in [
            ('root-return', host, '~ROOT_RETURN~'),
            ('child-return', child, '~CHILD_RETURN~'),
            ('root-second-return', host, '~ROOT_AGAIN~'),
        ]:
            xdo('windowactivate', '--sync', window)
            time.sleep(0.7)
            xdo('type', '--clearmodifiers', '--delay', 25, marker)
            capture(label + '.png')
            active = xdo('getactivewindow')
            assert active == window
            observations.append({'step': label, 'target': window, 'active': active})
        assert app.poll() is None
        (root / 'observations.json').write_text(json.dumps({
            'host': host, 'child': child, 'steps': observations,
            'application_exit_before_cleanup': app.poll(),
        }, indent=2) + '\n')
        print('Window identity and final active-window assertions passed; inspect text markers in screenshots.')
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
