"""Qualify the actual eight-pane basic example using the existing X11 drivers.

Build the example separately and provide an isolated X11 display. This runner
only owns optional Openbox startup and sequential driver invocation; all input
and behavioral assertions remain in editor_docking_x11.py and editor_x11.py.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import shutil
import subprocess
import sys

from with_openbox import window_manager


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="built native basic executable")
    parser.add_argument(
        "--start-openbox",
        action="store_true",
        help="own Openbox on a fresh isolated DISPLAY (omit for an existing WM)",
    )
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"not an executable file: {binary}")
    if not os.environ.get("DISPLAY"):
        parser.error("provide an isolated X11 DISPLAY, for example with xvfb-run")
    required_tools = ["xdotool", "xprop"]
    if args.start_openbox:
        required_tools.append("openbox")
    for tool in required_tools:
        if shutil.which(tool) is None:
            parser.error(f"required X11 tool is unavailable: {tool}")

    environment = os.environ | {
        "WINIT_UNIX_BACKEND": "x11",
        "WINIT_X11_SCALE_FACTOR": "1",
        "EGUI_DOCKSPACE_EDITOR_ROOT_REBIND": "1",
    }
    environment.pop("WAYLAND_DISPLAY", None)
    scripts = Path(__file__).resolve().parent
    print(
        f"editor-qualification: started={datetime.now(timezone.utc).isoformat()} "
        f"binary={binary}",
        flush=True,
    )
    with window_manager(start=args.start_openbox):
        for driver in ("editor_docking_x11.py", "editor_x11.py"):
            print(f"editor-qualification: running {driver}", flush=True)
            subprocess.run(
                [sys.executable, str(scripts / driver), str(binary)],
                env=environment,
                check=True,
            )
    print(
        f"editor-qualification: completed={datetime.now(timezone.utc).isoformat()} "
        "docking=passed lifecycle=passed root-rebind=passed",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"editor-qualification: failed: {error}", file=sys.stderr)
        sys.exit(1)
