import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, 'integration/egui-native-smoke')
from qualify_editor import wait_for_window_manager

out = Path(__file__).resolve().parent / 'reproduced-evidence'
out.mkdir(exist_ok=True)
manager = subprocess.Popen(['openbox', '--sm-disable'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
app = None
try:
    wait_for_window_manager(manager)
    app = subprocess.Popen(['/native-target/debug/examples/basic'], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    root = None
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        found = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(app.pid), '--name', '^Dockspace native editor$'], capture_output=True, text=True)
        if found.returncode == 0 and found.stdout.strip():
            root = found.stdout.splitlines()[0]
            break
        time.sleep(.03)
    assert root is not None
    def xdo(*arguments):
        subprocess.run(['xdotool', *arguments], check=True, timeout=5)
    def capture(name):
        subprocess.run(['import', '-window', root, str(out / name)], check=True)
    xdo('windowactivate', '--sync', root, 'windowsize', '--sync', root, '1180', '860')
    time.sleep(.3)
    capture('appearance-default.png')
    xdo('mousemove', '--window', root, '202', '12', 'click', '1')
    time.sleep(.2)
    capture('appearance-menu.png')
    xdo('mousemove', '--window', root, '200', '62', 'click', '1')
    time.sleep(.15)
    capture('appearance-editing.png')
    xdo('key', 'ctrl+a')
    xdo('type', '28')
    xdo('key', 'Return')
    time.sleep(.5)
    capture('appearance-typed.png')
    xdo('key', 'Escape')
    time.sleep(.2)
    capture('appearance-font28.png')
    print('Captured default, menu and 28-point font UI for visual inspection.')
finally:
    if app is not None:
        app.terminate()
        app.wait(timeout=5)
        print(app.stderr.read())
    manager.terminate()
    manager.wait(timeout=5)
