import hashlib
import json
from pathlib import Path
import subprocess
import time

root = Path('/tmp/dockspace-reload-final')
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
        xdo('type', '--clearmodifiers', '--delay', 20, '~RETAINED_CONTENT~')
        capture('document-edited.png')
        xdo('mousemove', '--window', host, 307, 34)
        xdo('click', 1)
        time.sleep(0.5)
        xdo('mousemove', '--window', host, 330, 83)
        time.sleep(0.7)
        capture('document-actions.png')
        xdo('mousemove', '--window', host, 604, 273)
        xdo('click', 1)
        time.sleep(0.8)
        capture('document-unloaded.png')
        xdo('mousemove', '--window', host, 253, 249)
        xdo('click', 1)
        time.sleep(0.7)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~AFTER_RELOAD~')
        capture('document-reloaded-typed.png')
        (root / 'identity.json').write_text(json.dumps({
            'binary_sha256': hashlib.sha256(Path('/native-target/debug/examples/basic').read_bytes()).hexdigest(),
            'source_head': subprocess.check_output(['git', '-C', '/Users/frankorz/codes/rust/egui_docking', 'rev-parse', 'HEAD'], text=True).strip(),
            'active_window': xdo('getactivewindow'),
            'host_window': host,
            'app_alive': app.poll() is None,
            'journey': 'edit Document; unload through inventory; click Reload content; type without clicking editor',
            'text_and_focus_verdict': 'inspect original screenshots separately from process/window assertions',
        }, indent=2) + '\n')
        assert xdo('getactivewindow') == host
        xdo('mousemove', '--window', host, 318, 170)
        xdo('click', 1)
        time.sleep(0.6)
        capture('document-close-request.png')
        xdo('mousemove', '--window', host, 125, 165)
        xdo('click', 1)
        time.sleep(0.7)
        capture('document-closed.png')
        xdo('mousemove', '--window', host, 307, 34)
        xdo('click', 1)
        time.sleep(0.5)
        xdo('mousemove', '--window', host, 330, 83)
        time.sleep(0.6)
        capture('document-reopen-menu.png')
        xdo('mousemove', '--window', host, 585, 85)
        xdo('click', 1)
        time.sleep(0.8)
        xdo('key', 'ctrl+End')
        xdo('type', '--clearmodifiers', '--delay', 20, '~AFTER_REOPEN~')
        capture('document-reopened-typed.png')
        assert xdo('getactivewindow') == host
        print('Reload and reopen completed; inspect text in the original screenshots.', flush=True)
        assert app.poll() is None
    finally:
        app.terminate()
        app.wait(timeout=10)
        manager.terminate()
        manager.wait(timeout=5)
