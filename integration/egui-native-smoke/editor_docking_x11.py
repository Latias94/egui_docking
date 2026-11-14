"""Exercise actual tab docking in the eight-pane native basic example.

Run in an isolated X11 display with a window manager and xdotool. Pass the built
basic executable. Coordinates come from read-only completed-pass diagnostics;
the application checks actual committed tab membership and presentation.
Each case starts a fresh editor and uses only physical mouse input.
The two drain cases also leave/re-enter the client area with primary held, drain
both contained tabs, and resize both surviving dividers from their hit padding.
"""

from __future__ import annotations

import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time


def xdotool(*arguments: object) -> str:
    return subprocess.check_output(
        ["xdotool", *(str(argument) for argument in arguments)],
        text=True,
        timeout=3,
    ).strip()


def leave_and_return(window: int, x: int, y: int) -> None:
    width, height = map(int, xdotool("getdisplaygeometry").split())
    geometry = dict(
        line.split("=", 1)
        for line in xdotool("getwindowgeometry", "--shell", window).splitlines()
    )
    left, top, right, bottom = (
        int(geometry["X"]), int(geometry["Y"]),
        int(geometry["X"]) + int(geometry["WIDTH"]),
        int(geometry["Y"]) + int(geometry["HEIGHT"]),
    )
    for outside_x, outside_y in ((1, height - 2), (width - 2, height - 2), (1, 1)):
        if not (left <= outside_x < right and top <= outside_y < bottom):
            xdotool("mousemove", outside_x, outside_y)
            time.sleep(0.15)
            xdotool("mousemove", "--window", window, x, y)
            time.sleep(0.15)
            return
    raise RuntimeError("the isolated display needs space outside the editor client area")


def run_case(binary: Path, case: str) -> None:
    environment = os.environ | {"EGUI_DOCKSPACE_DOCKING_CASE": case}
    process = subprocess.Popen(
        [str(binary), "--docking-smoke"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        env=environment,
    )
    assert process.stdout is not None
    lines: queue.Queue[str | None] = queue.Queue()

    def read_output() -> None:
        for line in process.stdout:
            lines.put(line)
        lines.put(None)

    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()
    completed = False
    held = False
    tab_drags = 0
    try:
        deadline = time.monotonic() + (45 if case.startswith("drain-") else 20)
        while time.monotonic() < deadline:
            try:
                line = lines.get(timeout=1)
            except queue.Empty:
                continue
            if line is None:
                break
            print(f"{case}: {line}", end="", flush=True)
            if line.startswith("editor-docking-select-target:"):
                window, x, y = map(int, line.split(":", 1)[1].split())
                xdotool("windowactivate", "--sync", window)
                xdotool("mousemove", "--window", window, x, y)
                xdotool("click", 1)
            elif line.startswith(("editor-docking-ready:", "editor-docking-resize-ready:")):
                is_tab = line.startswith("editor-docking-ready:")
                reenter = is_tab and tab_drags == 0 and case.startswith("drain-")
                tab_drags += int(is_tab)
                window, sx, sy, tx, ty = map(int, line.split(":", 1)[1].split())
                xdotool("windowactivate", "--sync", window)
                xdotool("mousemove", "--window", window, sx, sy)
                time.sleep(0.12)
                xdotool("mousedown", 1)
                held = True
                time.sleep(0.12)
                for step in range(1, 13):
                    x = round(sx + (tx - sx) * step / 12)
                    y = round(sy + (ty - sy) * step / 12)
                    xdotool(
                        "mousemove", "--window", window,
                        x, y,
                    )
                    time.sleep(0.025)
                    if reenter and step == 4:
                        leave_and_return(window, x, y)
                time.sleep(0.2)
                xdotool("mouseup", 1)
                held = False
            elif line.startswith("editor-docking-complete:"):
                completed = True
        status = process.wait(timeout=3)
        if status or not completed:
            raise RuntimeError(
                f"actual editor docking failed for {case}: exit={status}, completed={completed}"
            )
    finally:
        if held:
            try:
                xdotool("mouseup", 1)
            except (OSError, subprocess.SubprocessError) as error:
                print(f"{case}: could not release the mouse during cleanup: {error}", file=sys.stderr)
        try:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
        finally:
            reader.join(timeout=3)
            process.stdout.close()


def main() -> None:
    if len(sys.argv) != 2 or not os.environ.get("DISPLAY"):
        raise SystemExit("usage (under X11): python3 editor_docking_x11.py /path/to/basic")
    binary = Path(sys.argv[1]).resolve(strict=True)
    for case in [
        "outline", "console", "inspector", "preview", "profiler",
        "drain-inspector-first", "drain-preview-first",
    ]:
        run_case(binary, case)


if __name__ == "__main__":
    main()
