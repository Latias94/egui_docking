import pathlib
import subprocess
import time

root = pathlib.Path('/tmp/dockspace-ergonomics')
root.mkdir(exist_ok=True)
wm = subprocess.Popen(['openbox'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
with (root / 'after.stderr').open('w') as err:
    app = subprocess.Popen(['/native-target/production/official-caller-owned-demo'], stdout=subprocess.DEVNULL, stderr=err)
    try:
        window = None
        for _ in range(50):
            found = subprocess.run(['xdotool', 'search', '--name', 'official caller-owned run'], capture_output=True, text=True)
            if found.returncode == 0:
                window = found.stdout.splitlines()[-1]
                break
            time.sleep(0.2)
        if window is None:
            raise RuntimeError('The actual editor window did not appear')
        subprocess.run(['xdotool', 'windowactivate', '--sync', window], check=True, timeout=10)
        def point(x, y):
            subprocess.run(['xdotool', 'mousemove', '--window', window, str(x), str(y)], check=True)
        def capture(name):
            subprocess.run(['xdotool', 'mousemove', '1340', '940'], check=True)
            time.sleep(1)
            subprocess.run(['import', '-window', 'root', str(root / name)], check=True, timeout=15)
        time.sleep(3)
        capture('after.png')
        point(450, 85)
        subprocess.run(['xdotool', 'click', '1'], check=True)
        subprocess.run(['xdotool', 'type', '--clearmodifiers', '--delay', '40', '~ERGONOMICS~'], check=True)
        capture('edited.png')
        point(400, 42)
        subprocess.run(['xdotool', 'click', '1'], check=True)
        capture('closed.png')
        point(177, 12)
        subprocess.run(['xdotool', 'click', '1'], check=True)
        capture('restored.png')
        point(307, 200)
        time.sleep(2)
        subprocess.run(['import', '-window', 'root', str(root / 'splitter-tooltip.png')], check=True, timeout=15)
        name = subprocess.check_output(['xdotool', 'getwindowname', window], text=True).strip()
        assert 'rejected' not in name.lower(), name
        print('Actual X11 editor stayed live after text input, close and Restore clicks:', name)
    finally:
        app.terminate()
        app.wait(timeout=15)
        wm.terminate()
        wm.wait(timeout=10)
