from pathlib import Path
import json
import os
import subprocess
import time

root = Path(__file__).resolve().parent
out = root / 'evidence'
out.mkdir(exist_ok=True)

def xdo(*args):
    return subprocess.check_output(['xdotool', *map(str, args)], text=True, timeout=6).strip()

def wait_window(title):
    for _ in range(100):
        r = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^' + title + '$'], text=True, capture_output=True)
        if r.returncode == 0:
            return r.stdout.splitlines()[-1]
        time.sleep(.1)
    raise RuntimeError(title)

def state(nodes):
    # The recorder replaces a small JSON file on every frame.
    for _ in range(20):
        try:
            return json.loads(nodes.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            time.sleep(.05)
    raise RuntimeError('No recorded frame')

def snap(name, nodes=None):
    time.sleep(.35)
    subprocess.run(['import', '-window', 'root', str(out / (name + '.png'))], check=True, timeout=10)
    if nodes:
        (out / (name + '.json')).write_text(json.dumps(state(nodes), indent=2))
    print(name, flush=True)

def click_node(win, nodes, label, role=None, last=False):
    data = state(nodes)
    matches = [n for n in data['nodes'] if n['label'] == label and n['bounds'] and not n['disabled'] and (role is None or n['role'] == role)]
    node = max(matches, key=lambda n: n['bounds'][1]) if last else matches[0]
    a, b, c, d = node['bounds']
    # AccessKit bounds here are egui points; XTest is relative to client pixels.
    scale = data['pixels_per_point']
    xdo('mousemove', '--sync', '--window', win, int((a + c) * .5 * scale), int((b + d) * .5 * scale), 'click', 1)
    time.sleep(.4)
    return node

manager = subprocess.Popen(['openbox'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    time.sleep(.5)
    variants = [('egui-dark', []), ('egui-light', ['--light']), ('egui-large', ['--large']), ('egui-sentinel', ['--light', '--sentinel']), ('egui-zoom', ['--zoom']), ('egui-long', ['--long-title']), ('egui-last', ['--select-last']), ('egui-font36', ['--font36']), ('egui-long-zoom', ['--long-title', '--zoom'])]
    for name, args in variants:
        if os.environ.get('UI_AUDIT_ONLY') and name not in os.environ['UI_AUDIT_ONLY'].split(','):
            continue
        nodes = out / (name + '.json')
        env = dict(os.environ, UI_AUDIT_NODES=str(nodes))
        with (out / (name + '.stderr')).open('w') as errors:
            app = subprocess.Popen(['/native-target/debug/dockspace-ui-style-audit', *args], env=env, stdout=subprocess.DEVNULL, stderr=errors)
            try:
                win = wait_window('Dockspace UI style audit')
                xdo('windowmove', win, 0, 0)
                xdo('windowactivate', '--sync', win)
                xdo('mousemove', '--window', win, 30, 550)
                time.sleep(1.2)
                assert app.poll() is None
                snap(name + '-base', nodes)
                if name in ['egui-dark', 'egui-light', 'egui-sentinel']:
                    xdo('windowsize', win, 700, 620)
                    snap(name + '-narrow', nodes)
                    click_node(win, nodes, 'Show hidden tabs')
                    assert any(n['role'] == 'Menu' and n['label'] == 'Open tabs' for n in state(nodes)['nodes'])
                    snap(name + '-overflow', nodes)
                    xdo('key', 'Down')
                    snap(name + '-overflow-next', nodes)
                    xdo('key', 'Escape')
                    time.sleep(.2)
                    xdo('windowsize', win, 1280, 820)
                    time.sleep(.4)
                    click_node(win, nodes, 'Presentation commands', last=True)
                    assert any(n['label'] == 'Close Group' for n in state(nodes)['nodes'])
                    snap(name + '-commands', nodes)
                    xdo('key', 'Escape')
                    if name == 'egui-sentinel':
                        click_node(win, nodes, 'Native menu')
                        snap(name + '-native-menu', nodes)
                if name == 'egui-dark':
                    xdo('windowsize', win, 700, 620)
                    time.sleep(.4)
                    tab = click_node(win, nodes, 'Document.rs', 'Tab')
                    for count in range(80):
                        if state(nodes)['focus'] == tab['id']:
                            break
                        xdo('key', 'Tab')
                        time.sleep(.14)
                    else:
                        raise RuntimeError('Tab never received keyboard focus')
                    snap(name + '-end-before', nodes)
                    xdo('key', 'End')
                    snap(name + '-end-after', nodes)
                    xdo('key', 'Right')
                    snap(name + '-next-visible', nodes)
                    assert any(n['role'] == 'Tab' and n['label'] == 'History' and n['selected'] for n in state(nodes)['nodes'])
                    xdo('key', 'Home')
                    snap(name + '-home-visible', nodes)
                    assert any(n['role'] == 'Tab' and n['label'] == 'Document.rs' and n['selected'] for n in state(nodes)['nodes'])
                    xdo('windowsize', win, 1280, 820)
                    time.sleep(.4)
                    xdo('key', 'End')
                    snap(name + '-end-visible', nodes)
                    assert any(n['role'] == 'Tab' and n['label'] == 'Material settings and texture references' and n['selected'] for n in state(nodes)['nodes'])
                    xdo('key', 'Home')
                    time.sleep(.4)
                    # Tab through the actual focus order until a floating resize edge is focused.
                    for count in range(100):
                        xdo('key', 'Tab')
                        time.sleep(.12)
                        data = state(nodes)
                        focused = next((n for n in data['nodes'] if n['id'] == data['focus']), {})
                        if focused.get('label') == 'Resize floating right edge':
                            break
                    else:
                        raise RuntimeError('Resize edge never received keyboard focus')
                    xdo('mousemove', '--window', win, 20, 560)
                    snap(name + '-edge-before', nodes)
                    xdo('key', 'Right')
                    snap(name + '-edge-after', nodes)
                if name == 'egui-last':
                    xdo('windowsize', win, 700, 620)
                    snap(name + '-narrow', nodes)
                assert app.poll() is None
            finally:
                app.terminate()
                app.wait(timeout=10)
    for name, args in [('imgui-dark', []), ('imgui-light', ['--light']), ('imgui-large', ['--large'])]:
        if os.environ.get('UI_AUDIT_ONLY') and name not in os.environ['UI_AUDIT_ONLY'].split(','):
            continue
        with (out / (name + '.stderr')).open('w') as errors:
            app = subprocess.Popen(['/native-target/imgui-ui-audit', *args], stdout=subprocess.DEVNULL, stderr=errors)
            try:
                win = wait_window('Dear ImGui docking audit')
                xdo('windowmove', win, 0, 0)
                xdo('windowactivate', '--sync', win)
                xdo('mousemove', '--window', win, 30, 550)
                time.sleep(1.2)
                snap(name + '-base')
                if name == 'imgui-dark':
                    xdo('mousemove', '--sync', '--window', win, 253, 10, 'click', 1)
                    snap(name + '-menu')
            finally:
                app.terminate()
                app.wait(timeout=10)
finally:
    manager.terminate()
    manager.wait(timeout=5)
