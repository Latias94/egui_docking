import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-window-close-final')
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
        observations = []
        def record(step, target):
            active = xdo('getactivewindow')
            observations.append({'time': time.time(), 'step': step,
                                 'target': target, 'active': active})
            (root / 'observations.json').write_text(json.dumps({
                'host': host, 'child': child, 'steps': observations,
                'application_exit_before_cleanup': app.poll(),
            }, indent=2) + '\n')
            assert active == target, observations[-1]
            assert app.poll() is None
        xdo('windowactivate', host)
        time.sleep(0.7)
        capture('root-first-return.png')
        record('root-first-return', host)
        xdo('windowmove', child, 940, 120)
        time.sleep(0.4)
        for label, window, point, marker in [
            ('root-established', host, (400, 350), '~ROOT_ESTABLISHED~'),
            ('child-established', child, (250, 100), '~CHILD_ESTABLISHED~'),
        ]:
            xdo('mousemove', '--window', window, *point)
            xdo('click', 1)
            time.sleep(0.4)
            xdo('key', 'ctrl+End')
            xdo('type', '--clearmodifiers', '--delay', 25, marker)
            capture(label + '.png')
            record(label, window)
        xdo('mousemove', 20, 20)
        for label, window, marker in [
            ('root-return', host, '~ROOT_RETURN~'),
            ('child-return', child, '~CHILD_RETURN~'),
            ('root-second-return', host, '~ROOT_AGAIN~'),
            ('child-second-return', child, '~CHILD_AGAIN~'),
        ]:
            xdo('key', 'alt+Tab')
            time.sleep(0.7)
            xdo('type', '--clearmodifiers', '--delay', 25, marker)
            capture(label + '.png')
            record(label, window)
        xdo('key', 'alt+Tab')
        time.sleep(0.5)
        record('before-document-close', host)
        xdo('mousemove', '--window', host, 318, 170)
        xdo('click', 1)
        time.sleep(0.7)
        capture('document-close-request.png')
        record('document-close-request', host)
        xdo('mousemove', '--window', host, 125, 165)
        xdo('click', 1)
        time.sleep(0.7)
        xdo('type', '--clearmodifiers', '--delay', 25, '~AFTER_DOC_APPROVAL~')
        capture('document-closed.png')
        record('document-closed', host)
        xdo('mousemove', '--window', host, 400, 350)
        xdo('click', 1)
        time.sleep(0.4)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 25, '~HISTORY_ACTIVE~')
        capture('history-edited.png')
        record('history-edited', host)
        xdo('mousemove', '--window', child, 202, 14)
        xdo('click', 1)
        time.sleep(0.7)
        capture('native-pane-close-request.png')
        record('native-pane-close-request', child)
        xdo('mousemove', '--window', host, 125, 165)
        xdo('click', 1)
        time.sleep(0.8)
        capture('native-pane-closed.png')
        record('native-pane-closed', host)
        assert find_window('Dockspace panel') == child
        xdo('mousemove', 20, 20)
        xdo('key', 'alt+Tab')
        time.sleep(0.7)
        capture('native-successor-return.png')
        record('native-successor-return', child)
        print('Creation return, four Alt+Tab edits and both close-decision journeys passed OS assertions. Inspect pane selection and text in original screenshots.', flush=True)
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
