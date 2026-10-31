"""Finite X11 input owner for the native eight-pane example's --smoke workflow.

Run in an isolated X11 test display with a window manager and xdotool. Build
the native basic example separately, then pass its executable as the sole
argument. Diagnostic rectangles only locate real input; the app checks current
presentation, stable pane identity, exact text and lifecycle quiescence.
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
    markers = iter(["ROOT_EDIT", "CHILD_EDIT", "BACK_EDIT", "RECOVERED_EDIT"])
    expected = next(markers)
    children: list[str] = []
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
                if kind == "root":
                    for retired in children:
                        assert_destroyed(retired)
                target = window(process.pid, kind)
                if kind == "child":
                    children.append(target)
                xdotool("windowactivate", "--sync", target)
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
            elif "editor-smoke: close-child;" in line:
                target = window(process.pid, "child")
                children.append(target)
                xdotool("windowactivate", "--sync", target)
                # The window manager generates a real WM_DELETE_WINDOW request.
                xdotool("key", "--clearmodifiers", "alt+F4")
            elif "editor-smoke: complete;" in line:
                if expected != "COMPLETE":
                    raise RuntimeError("the application completed before all four OS edits")
                complete = True
        code = process.wait(timeout=5)
        if not complete or code != 0:
            raise RuntimeError(
                f"editor workflow incomplete={not complete}, process exit={code}"
            )
        for retired in children:
            assert_destroyed(retired)
        print("editor-x11: four real focused edits and two child-window lifetimes verified")
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
