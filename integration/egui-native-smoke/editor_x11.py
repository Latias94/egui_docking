"""Finite X11 input owner for the native eight-pane example's --smoke workflow.

Run in an isolated X11 test display with a window manager and xdotool. Build
the native basic example separately, then pass its executable as the sole
argument. Diagnostic rectangles only locate real input; the app checks current
presentation, stable pane identity, exact text and lifecycle quiescence.
Six edits (seven with serde) and the second child close use real X11 input.
Close/Allow/reopen, lock/rejected move/select/unlock, Default, JSON capture and
restore, content unload/reload and explicit selection are programmatic.
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


def window(pid: int, kind: str) -> str:
    title = "Dockspace panel" if kind == "child" else "Dockspace native editor"
    matches = xdotool(
        "search", "--all", "--onlyvisible", "--pid", str(pid), "--name", f"^{title}$"
    ).splitlines()
    if len(matches) != 1:
        raise RuntimeError(f"expected one {kind} window for process {pid}, got {matches}")
    return matches[0]


def assert_destroyed(window_id: str) -> None:
    result = subprocess.run(
        ["xdotool", "getwindowgeometry", window_id],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=5,
        check=False,
    )
    if result.returncode == 0:
        raise RuntimeError(f"retired child window {window_id} still exists")


def run(binary: Path) -> None:
    if not os.environ.get("DISPLAY"):
        raise RuntimeError("this finite physical-input workflow requires DISPLAY")
    xdotool("getdisplaygeometry")
    process = subprocess.Popen(
        [str(binary), "--smoke"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    assert process.stdout is not None
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
    }
    edits: list[str] = []
    profile: str | None = None
    markers = iter(edits)
    expected = "PROFILE"
    accepted: list[str] = []
    programmatic = iter([])
    expected_programmatic = "PROFILE"
    children: list[str] = []
    os_close_sent = False
    complete = False
    deadline = time.monotonic() + 75
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
                    edits.append("RESTORED_EDIT")
                    operations.extend([
                        "capture-preset", "restore-close-requested", "restore-allow-close",
                        "restore-closed", "unload", "document-restored", "missing-placeholder",
                        "reload", "restore-select",
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
                r"editor-smoke: input=(\w+) item=(\d+) window=(root|child) x=([\d.]+) y=([\d.]+)",
                line,
            )
            if match:
                marker, item, kind, x, y = match.groups()
                if marker != expected or edit_items.get(marker) != item:
                    raise RuntimeError(
                        f"expected {expected} and its exact item, got {marker} item={item}"
                    )
                expected_kind = "child" if marker == "CHILD_EDIT" else "root"
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
                if marker == "RESTORED_EDIT" and expected_programmatic != "final-quiescence":
                    raise RuntimeError("RESTORED_EDIT preceded missing placeholder/reload/select")
                if kind == "root":
                    for retired in children:
                        assert_destroyed(retired)
                target = window(process.pid, kind)
                if kind == "child":
                    children.append(target)
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
                xdotool("key", "--clearmodifiers", "ctrl+End")
                xdotool("type", "--clearmodifiers", "--delay", "1", marker)
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
                elif operation == "final-quiescence":
                    expected_edits = edits
                else:
                    expected_edits = edits[:6]
                if accepted != expected_edits:
                    raise RuntimeError(
                        f"programmatic {operation} preceded its required OS edits"
                    )
                expected_programmatic = next(programmatic, "COMPLETE")
            elif "editor-smoke: complete;" in line:
                if expected != "COMPLETE" or accepted != edits:
                    raise RuntimeError("the application completed before all required OS edits")
                if expected_programmatic != "COMPLETE":
                    raise RuntimeError(
                        "the application completed before Default/restore quiescence"
                    )
                if not os_close_sent or len(children) != 2:
                    raise RuntimeError("the workflow did not exercise two child lifetimes")
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
            "two destroyed child windows, programmatic close/Allow/reopen and "
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
