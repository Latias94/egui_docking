"""Finite X11 input owner for the native eight-pane example's --smoke workflow.

Run in an isolated X11 test display with a window manager and xdotool. Build
the native basic example separately, then pass its executable as the sole
argument. Diagnostic rectangles only locate real input; the app checks current
presentation, stable pane identity, exact text and lifecycle quiescence.
Five edits and the second child close use real X11 input. Content close/Allow,
reopening and arrangement lock/rejected move/select/unlock are programmatic.
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
    edits = ["ROOT_EDIT", "CHILD_EDIT", "BACK_EDIT", "RECOVERED_EDIT", "REOPENED_EDIT"]
    markers = iter(edits)
    expected = next(markers)
    accepted: list[str] = []
    programmatic = iter(
        [
            "close-requested",
            "allow-close",
            "closed",
            "reopen",
            "reopen-focus",
            "lock",
            "locked-move-rejected",
            "select-while-locked",
            "unlock",
            "quiescent",
        ]
    )
    expected_programmatic = next(programmatic)
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
            match = re.search(
                r"editor-smoke: input=(\w+) window=(root|child) x=([\d.]+) y=([\d.]+)",
                line,
            )
            if match:
                marker, kind, x, y = match.groups()
                if marker != expected:
                    raise RuntimeError(f"expected {expected}, got {marker}")
                expected_kind = "child" if marker == "CHILD_EDIT" else "root"
                if kind != expected_kind:
                    raise RuntimeError(f"expected {marker} in {expected_kind}, got {kind}")
                if marker == "REOPENED_EDIT" and expected_programmatic != "lock":
                    raise RuntimeError(
                        "the fifth OS edit was requested before close/Allow/reopen"
                    )
                if kind == "root":
                    for retired in children:
                        assert_destroyed(retired)
                target = window(process.pid, kind)
                if kind == "child":
                    children.append(target)
                xdotool("windowactivate", "--sync", target)
                if marker == "REOPENED_EDIT":
                    # The actual TextEdit has already confirmed restored focus.
                    # Its geometric center may lie behind the surviving floating
                    # Preview; typing here verifies focus without a new click.
                    print("editor-x11: fifth edit uses the restored widget focus", flush=True)
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
                marker = line.strip().split("focused=", 1)[1]
                if marker != expected:
                    raise RuntimeError(f"expected focused {expected}, got {marker}")
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
                    or item != "4"
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
                expected_edits = (
                    edits[:4]
                    if operation in {
                        "close-requested", "allow-close", "closed", "reopen", "reopen-focus"
                    }
                    else edits
                )
                if accepted != expected_edits:
                    raise RuntimeError(
                        f"programmatic {operation} preceded its required OS edits"
                    )
                expected_programmatic = next(programmatic, "COMPLETE")
            elif "editor-smoke: complete;" in line:
                if expected != "COMPLETE" or accepted != edits:
                    raise RuntimeError("the application completed before all five OS edits")
                if expected_programmatic != "COMPLETE":
                    raise RuntimeError(
                        "the application completed before close/reopen/lock quiescence"
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
            "editor-x11: five real focused edits on the same Inspector item, "
            "two destroyed child windows, programmatic close/Allow/reopen and "
            "lock/rejected move/select/unlock with final quiescence verified"
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
