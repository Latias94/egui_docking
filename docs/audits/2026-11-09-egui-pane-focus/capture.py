from pathlib import Path
import json
import os
import subprocess
import time

out = Path(__file__).resolve().parent / 'reproduced-evidence'
out.mkdir(exist_ok=True)


def xdo(*args):
    return subprocess.check_output(['xdotool', *map(str, args)], text=True, timeout=6).strip()


def state(path):
    for _ in range(20):
        try:
            return json.loads(path.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            time.sleep(.05)
    raise RuntimeError('No recorded frame')


def snapshot(name, path):
    time.sleep(.5)
    data = state(path)
    subprocess.run(['import', '-window', 'root', str(out / (name + '.png'))], check=True, timeout=10)
    (out / (name + '.json')).write_text(json.dumps(data, indent=2))
    print(name, flush=True)
    return data


def cues(data):
    return [r['bounds'] for r in data['rects']
            if r['fill'] == data['selection_color'] and abs(r['bounds'][3] - r['bounds'][1] - 2.0) < .01]


def selected(data):
    return {n['label'] for n in data['nodes'] if n['role'] == 'Tab' and n['selected']}


def assert_cue(data, title):
    tab = next(n for n in data['nodes'] if n['role'] == 'Tab' and n['label'] == title)
    bounds = tab['bounds']
    lines = cues(data)
    assert len(lines) == 1, (title, lines)
    assert abs(lines[0][3] - bounds[3]) < .01, (title, lines, bounds)
    assert lines[0][0] <= bounds[0] and lines[0][2] >= bounds[2]
    assert data['focus'] != tab['id'], 'Content focus must differ from the tab widget'


def click_editor(window, path, region):
    data = state(path)
    editors = [t for t in data['texts'] if t['text'].startswith('Select an object to inspect')]
    if region == 'main':
        text = max(editors, key=lambda t: t['clip'][2] - t['clip'][0])
    elif region == 'floating':
        text = next(t for t in editors if 400 < t['pos'][0] < 500 and t['pos'][1] > 300)
    else:
        text = max(editors, key=lambda t: t['pos'][0])
    scale = data['pixels_per_point']
    x, y = text['pos']
    xdo('mousemove', '--sync', '--window', window, int((x + 8) * scale), int((y + 6) * scale), 'click', 1)


manager = subprocess.Popen(['openbox'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    time.sleep(.5)
    for name, args in [('dark', []), ('light', ['--light']), ('zoom', ['--zoom'])]:
        path = out / (name + '-live.json')
        with (out / (name + '.stderr')).open('w') as errors:
            app = subprocess.Popen(['/native-target/debug/dockspace-ui-style-audit', *args],
                                   env=dict(os.environ, UI_AUDIT_NODES=str(path)),
                                   stdout=subprocess.DEVNULL, stderr=errors)
            try:
                for _ in range(100):
                    found = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^Dockspace UI style audit$'],
                                           text=True, capture_output=True)
                    if found.returncode == 0:
                        window = found.stdout.splitlines()[-1]
                        break
                    time.sleep(.1)
                else:
                    raise RuntimeError('Fixture window did not appear')
                xdo('windowmove', window, 0, 0)
                xdo('windowactivate', '--sync', window)
                xdo('mousemove', '--window', window, 20, 700)
                time.sleep(1.2)
                initial = snapshot(name + '-selected', path)
                assert not cues(initial)
                click_editor(window, path, 'main')
                main = snapshot(name + '-editor', path)
                if not cues(main):
                    probe = subprocess.Popen(['xmessage', '-geometry', '240x80+1100+880', 'Focus probe'])
                    try:
                        time.sleep(.6)
                        xdo('windowactivate', '--sync', window)
                        probe.terminate()
                        probe.wait(timeout=5)
                        main = snapshot(name + '-refocused', path)
                    finally:
                        if probe.poll() is None:
                            probe.terminate()
                            probe.wait(timeout=5)
                assert_cue(main, 'Document.rs')
                assert selected(main) == selected(initial)
                if name != 'zoom':
                    click_editor(window, path, 'floating')
                    floating = snapshot(name + '-floating', path)
                    assert_cue(floating, 'Inspector')
                    assert selected(floating) == selected(initial)
                click_editor(window, path, 'external')
                external = snapshot(name + '-external', path)
                assert not cues(external)
                assert selected(external) == selected(initial)
                assert app.poll() is None
            finally:
                app.terminate()
                app.wait(timeout=10)
finally:
    manager.terminate()
    manager.wait(timeout=5)
