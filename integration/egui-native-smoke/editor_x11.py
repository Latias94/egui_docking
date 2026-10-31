"""Finite X11 input owner for the native eight-pane example's --smoke workflow.

Run in an isolated X11 test display with a window manager and xdotool. Build
the native basic example separately, then pass its executable as the sole
argument. Diagnostic rectangles only locate real input; the app checks current
presentation, stable pane identity, exact text and lifecycle quiescence.
Six edits (eight with serde) and the second child close use real X11 input.
Close/Allow/reopen, lock/rejected move/select/unlock, Default, JSON capture and
restore, content unload/reload and explicit selection are programmatic. Serde
also replaces two simultaneously live children, verifies their old X11 IDs
are destroyed, edits a fresh child, then docks both replacement children back.
"""

from __future__ import annotations

import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import threading
import time


def xdotool(*arguments: str) -> str:
    return subprocess.check_output(
        ["xdotool", *arguments], text=True, stderr=subprocess.PIPE, timeout=5
    ).strip()


def windows(pid: int, kind: str) -> set[str]:
    title = "Dockspace panel" if kind == "child" else "Dockspace native editor"
    result = subprocess.run(
        ["xdotool", "search", "--all", "--onlyvisible", "--pid", str(pid),
         "--name", f"^{title}$"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=5,
        check=False,
    )
    if result.returncode == 1 and not result.stdout.strip():
        return set()
    result.check_returncode()
    return set(result.stdout.splitlines())


def window(pid: int, kind: str) -> str:
    matches = windows(pid, kind)
    if len(matches) != 1:
        raise RuntimeError(f"expected one {kind} window for process {pid}, got {matches}")
    return next(iter(matches))


def assert_destroyed(window_id: str) -> None:
    result = subprocess.run(
        ["xdotool", "getwindowgeometry", window_id],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        timeout=5,
        check=False,
    )
    if result.returncode == 0:
        raise RuntimeError(f"retired child window {window_id} still exists")
    if not any(reason in result.stderr for reason in ("BadWindow", "BadDrawable")):
        raise RuntimeError(f"could not verify destruction of {window_id}: {result.stderr.strip()}")


def run(binary: Path) -> None:
    if not os.environ.get("DISPLAY"):
        raise RuntimeError("this finite physical-input workflow requires DISPLAY")
    xdotool("getdisplaygeometry")
    process = subprocess.Popen(
        [str(binary), "--smoke"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    assert process.stdout is not None
    assert process.stdin is not None
    lines: queue.Queue[str | None] = queue.Queue()

    def read_output() -> None:
        for line in process.stdout:
            lines.put(line)
        lines.put(None)

    threading.Thread(target=read_output, daemon=True).start()
    edit_items = {
        "ROOT_EDIT": "4",
        "CHILD_EDIT": "4",
        "BACK_EDIT": "4",
        "RECOVERED_EDIT": "4",
        "REOPENED_EDIT": "4",
        "RESET_EDIT": "2",
        "RESTORED_EDIT": "4",
        "NATIVE_RESTORED_EDIT": "4",
    }
    edits: list[str] = []
    profile: str | None = None
    markers = iter(edits)
    expected = "PROFILE"
    accepted: list[str] = []
    programmatic = iter([])
    expected_programmatic = "PROFILE"
    children: list[str] = []
    old_children: list[str] = []
    fresh_children: list[str] = []
    restore_destroyed: set[str] = set()
    docked_destroyed: set[str] = set()
    input_target: str | None = None
    os_close_sent = False
    complete = False
    deadline = time.monotonic() + 105
    try:
        while time.monotonic() < deadline:
            try:
                line = lines.get(timeout=min(1, max(0, deadline - time.monotonic())))
            except queue.Empty:
                if process.poll() is not None:
                    break
                continue
            if line is None:
                break
            print(line, end="", flush=True)
            if "editor-smoke: FAIL" in line:
                raise RuntimeError(line.strip())
            if "editor-smoke: profile=" in line:
                match = re.search(r"editor-smoke: profile=(basic|serde) edits=(\d+)", line)
                if match is None or profile is not None:
                    raise RuntimeError("expected one exact basic/serde workflow profile")
                profile, count = match.groups()
                edits = [
                    "ROOT_EDIT", "CHILD_EDIT", "BACK_EDIT", "RECOVERED_EDIT",
                    "REOPENED_EDIT", "RESET_EDIT",
                ]
                operations = [
                    "close-requested", "allow-close", "closed", "reopen", "reopen-focus",
                    "lock", "locked-move-rejected", "select-while-locked", "unlock",
                    "quiescent", "default-reset", "default-presented", "default-select",
                ]
                if profile == "serde":
                    edits.extend(["RESTORED_EDIT", "NATIVE_RESTORED_EDIT"])
                    operations.extend([
                        "capture-preset", "restore-close-requested", "restore-allow-close",
                        "restore-closed", "unload", "document-restored", "missing-placeholder",
                        "reload", "restore-select",
                        "multi-layout", "multi-inspector-presented", "multi-children-ready",
                        "capture-multi-preset", "multi-restore-requested",
                        "multi-document-restored", "multi-children-presented",
                        "multi-inspector-docked-back", "multi-assets-docked-back",
                    ])
                operations.append("final-quiescence")
                if int(count) != len(edits):
                    raise RuntimeError("the workflow profile declared the wrong OS edit count")
                markers = iter(edits)
                expected = next(markers)
                programmatic = iter(operations)
                expected_programmatic = next(programmatic)
                continue
            match = re.search(
                r"editor-smoke: input=(\w+) item=(\d+) window=(root|child) "
                r"x=([-\d.]+) y=([-\d.]+) window_id=(\d+) "
                r"left=([-\d.]+) top=([-\d.]+) right=([-\d.]+) bottom=([-\d.]+)",
                line,
            )
            if match:
                marker, item, kind, x, y, target, left, top, right, bottom = match.groups()
                if marker != expected or edit_items.get(marker) != item:
                    raise RuntimeError(
                        f"expected {expected} and its exact item, got {marker} item={item}"
                    )
                expected_kind = (
                    "child" if marker in {"CHILD_EDIT", "NATIVE_RESTORED_EDIT"} else "root"
                )
                if kind != expected_kind:
                    raise RuntimeError(f"expected {marker} in {expected_kind}, got {kind}")
                if marker == "REOPENED_EDIT" and expected_programmatic != "lock":
                    raise RuntimeError(
                        "the fifth OS edit was requested before close/Allow/reopen"
                    )
                if marker == "RESET_EDIT" and expected_programmatic != (
                    "capture-preset" if profile == "serde" else "final-quiescence"
                ):
                    raise RuntimeError("RESET_EDIT preceded fresh Default presentation/select")
                if marker == "RESTORED_EDIT" and expected_programmatic != "multi-layout":
                    raise RuntimeError("RESTORED_EDIT preceded missing placeholder/reload/select")
                if marker == "NATIVE_RESTORED_EDIT":
                    if expected_programmatic != "multi-inspector-docked-back":
                        raise RuntimeError("the eighth edit preceded both fresh Child first-live outputs")
                    if not fresh_children or target != fresh_children[0]:
                        raise RuntimeError("the eighth edit did not target the fresh Inspector child")
                    if restore_destroyed != set(old_children):
                        raise RuntimeError("the eighth edit preceded destruction of both old children")
                if kind == "root":
                    for retired in children:
                        assert_destroyed(retired)
                if target not in windows(process.pid, kind):
                    raise RuntimeError(
                        f"actual widget window {target} is not a live {kind} owned by the app"
                    )
                if marker == "CHILD_EDIT":
                    children.append(target)
                if not (
                    float(left) < float(x) < float(right)
                    and float(top) < float(y) < float(bottom)
                ):
                    raise RuntimeError(f"{marker} input point lies outside the actual widget bounds")
                input_target = target
                xdotool("windowactivate", "--sync", target)
                if marker in {"REOPENED_EDIT", "RESET_EDIT", "RESTORED_EDIT"}:
                    # Wait for focused=<marker> from the actual TextEdit. A click
                    # here would replace the focus transfer being verified; its
                    # center may also lie behind the surviving floating Preview.
                    print(
                        f"editor-x11: {marker} item={item} waits for actual widget focus without clicking",
                        flush=True,
                    )
                    continue
                local_x, local_y = round(float(x)), round(float(y))
                xdotool("mousemove", "--window", target, str(local_x), str(local_y))
                geometry = subprocess.check_output(
                    ["xwininfo", "-id", target], text=True, timeout=5
                )
                origin = tuple(
                    int(re.search(rf"Absolute upper-left {axis}:\s*(-?\d+)", geometry)[1])
                    for axis in ["X", "Y"]
                )
                pointer = dict(
                    part.split("=", 1)
                    for part in xdotool("getmouselocation", "--shell").splitlines()
                )
                if (int(pointer["X"]), int(pointer["Y"])) != (
                    origin[0] + local_x,
                    origin[1] + local_y,
                ):
                    raise RuntimeError(
                        "the real X11 pointer did not reach the declared content point: "
                        f"origin={origin}, pointer={pointer}, local=({x}, {y})"
                    )
                if marker == "ROOT_EDIT":
                    # Let ordinary Presented settle without another input
                    # edge, then exceed the adapter's 250 ms ingress-age limit.
                    time.sleep(0.4)
                xdotool("click", "1")
            elif "editor-smoke: focused=" in line:
                match = re.search(r"editor-smoke: focused=(\w+) item=(\d+)", line)
                if match is None:
                    raise RuntimeError("focused diagnostic omitted its stable item")
                marker, item = match.groups()
                if marker != expected or edit_items.get(marker) != item:
                    raise RuntimeError(f"expected focused {expected}, got {marker} item={item}")
                if input_target is None or xdotool("getwindowfocus") != input_target:
                    raise RuntimeError(f"{marker} widget focus did not match the real OS focus")
                xdotool("key", "--clearmodifiers", "ctrl+End")
                xdotool("type", "--clearmodifiers", "--delay", "1", marker)
                input_target = None
                expected = next(markers, "COMPLETE")
            elif "editor-smoke: accepted real focused edit" in line:
                match = re.search(r"accepted real focused edit (\w+) item=(\d+)", line)
                if match is None:
                    raise RuntimeError(
                        "the accepted OS edit omitted its stable item identity"
                    )
                marker, item = match.groups()
                if (
                    len(accepted) >= len(edits)
                    or marker != edits[len(accepted)]
                    or edit_items.get(marker) != item
                ):
                    raise RuntimeError(f"unexpected accepted OS edit: {marker} item={item}")
                accepted.append(marker)
            elif "editor-smoke: close-child;" in line:
                if os_close_sent or accepted != edits[:3]:
                    raise RuntimeError("the OS child close did not follow the first three edits")
                target = window(process.pid, "child")
                children.append(target)
                xdotool("windowactivate", "--sync", target)
                # The window manager generates a real WM_DELETE_WINDOW request.
                xdotool("key", "--clearmodifiers", "alt+F4")
                os_close_sent = True
            elif "editor-smoke: programmatic=" in line:
                operation = line.split("programmatic=", 1)[1].split()[0]
                if operation != expected_programmatic:
                    raise RuntimeError(
                        f"expected programmatic {expected_programmatic}, got {operation}"
                    )
                if operation in {
                    "close-requested", "allow-close", "closed", "reopen", "reopen-focus"
                }:
                    expected_edits = edits[:4]
                elif operation in {
                    "lock", "locked-move-rejected", "select-while-locked", "unlock",
                    "quiescent", "default-reset", "default-presented", "default-select",
                }:
                    expected_edits = edits[:5]
                elif operation in {
                    "final-quiescence", "multi-inspector-docked-back", "multi-assets-docked-back"
                }:
                    expected_edits = edits
                elif operation.startswith("multi-") or operation == "capture-multi-preset":
                    expected_edits = edits[:7]
                else:
                    expected_edits = edits[:6]
                if accepted != expected_edits:
                    raise RuntimeError(
                        f"programmatic {operation} preceded its required OS edits"
                    )
                if operation == "multi-inspector-presented":
                    match = re.search(r"window_id=(\d+) first-live=true quiescent=true", line)
                    if match is None or match[1] not in windows(process.pid, "child"):
                        raise RuntimeError("Inspector first-live lacked its real OS child")
                elif operation == "multi-children-ready":
                    match = re.search(
                        r"window_ids=(\d+),(\d+) first-live=true quiescent=true registry=8",
                        line,
                    )
                    if match is None:
                        raise RuntimeError("both old children need exact presented OS window IDs")
                    old_children = list(match.groups())
                    if len(set(old_children)) != 2 or set(old_children) & set(children):
                        raise RuntimeError("the multi-surface predecessors reused earlier window IDs")
                    if windows(process.pid, "child") != set(old_children):
                        raise RuntimeError("the preset did not have two simultaneously live OS children")
                    children.extend(old_children)
                elif operation == "capture-multi-preset":
                    if not old_children or windows(process.pid, "child") != set(old_children):
                        raise RuntimeError("both predecessor windows must still be alive before restore")
                    for predecessor in old_children:
                        xdotool("getwindowgeometry", predecessor)
                    process.stdin.write("multi-children-observed\n")
                    process.stdin.flush()
                    print(
                        f"editor-x11: observed two live predecessors {','.join(old_children)}; restore released",
                        flush=True,
                    )
                elif operation == "multi-restore-requested":
                    if f"old-window_ids={','.join(old_children)} observed-alive=true" not in line:
                        raise RuntimeError("restore did not replace the exact observed predecessors")
                elif operation == "multi-document-restored":
                    if (
                        "logically-committed=true registry=8 tabs=preserved "
                        "selected=preserved fresh-presented=required"
                    ) not in line:
                        raise RuntimeError("DocumentRestored did not check full logical tabs/selection")
                elif operation == "multi-children-presented":
                    match = re.search(
                        r"window_ids=(\d+),(\d+) first-live=true quiescent=true registry=8 "
                        r"tabs=preserved selected=preserved",
                        line,
                    )
                    if match is None:
                        raise RuntimeError("replacement first-live omitted its exact child window IDs")
                    if len(old_children) != 2:
                        raise RuntimeError("replacement first-live preceded the two observed predecessors")
                    fresh_children = list(match.groups())
                    if len(set(fresh_children)) != 2 or set(fresh_children) & set(children):
                        raise RuntimeError("restore did not create two fresh, distinct OS children")
                    for predecessor in old_children:
                        assert_destroyed(predecessor)
                        restore_destroyed.add(predecessor)
                        print(
                            f"editor-x11: destroyed predecessor window_id={predecessor} before fresh-child input",
                            flush=True,
                        )
                    if windows(process.pid, "child") != set(fresh_children):
                        raise RuntimeError("both fresh Child windows must really exist after first-live")
                    children.extend(fresh_children)
                elif operation in {
                    "multi-inspector-docked-back", "multi-assets-docked-back"
                }:
                    index = 0 if operation == "multi-inspector-docked-back" else 1
                    if len(fresh_children) != 2:
                        raise RuntimeError("Dock Back preceded the two replacement children")
                    assert_destroyed(fresh_children[index])
                    docked_destroyed.add(fresh_children[index])
                elif operation == "final-quiescence" and profile == "serde":
                    if restore_destroyed != set(old_children) or docked_destroyed != set(fresh_children):
                        raise RuntimeError("final quiescence preceded real replacement and Dock Back destruction")
                    if windows(process.pid, "child"):
                        raise RuntimeError("final quiescence retained a real OS child")
                expected_programmatic = next(programmatic, "COMPLETE")
            elif "editor-smoke: complete;" in line:
                if expected != "COMPLETE" or accepted != edits:
                    raise RuntimeError("the application completed before all required OS edits")
                if expected_programmatic != "COMPLETE":
                    raise RuntimeError(
                        "the application completed before Default/restore quiescence"
                    )
                lifetimes = 6 if profile == "serde" else 2
                if not os_close_sent or len(children) != lifetimes or len(set(children)) != lifetimes:
                    raise RuntimeError(f"the workflow did not exercise {lifetimes} distinct child lifetimes")
                complete = True
        code = process.wait(timeout=5)
        if not complete or code != 0:
            raise RuntimeError(
                f"editor workflow incomplete={not complete}, process exit={code}"
            )
        for retired in children:
            assert_destroyed(retired)
        print(
            f"editor-x11: {len(edits)} real focused edits with exact Inspector/Document items, "
            f"{len(children)} destroyed child windows, programmatic close/Allow/reopen and "
            f"lock/rejected move/select/unlock, Default and {profile} profile with final quiescence verified"
        )
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: editor_x11.py /path/to/native/basic")
    run(Path(sys.argv[1]).resolve(strict=True))
