"""Run a command with an owned Openbox on an isolated, caller-supplied DISPLAY."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import os
import re
import subprocess
import sys
import time
from collections.abc import Iterator


def wait_for_window_manager(process: subprocess.Popen | None) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise RuntimeError("Openbox exited before publishing its X11 work area")
        properties = subprocess.check_output(
            ["xprop", "-root", "_NET_SUPPORTING_WM_CHECK", "_NET_WORKAREA"],
            text=True,
            timeout=3,
        )
        if (
            re.search(r"window id # 0x0*[1-9a-fA-F][0-9a-fA-F]*", properties)
            and re.search(r"_NET_WORKAREA\(CARDINAL\) = [0-9]", properties)
        ):
            return
        time.sleep(0.05)
    raise RuntimeError("the isolated X11 display has no ready EWMH window manager")


@contextmanager
def window_manager(*, start: bool = True) -> Iterator[None]:
    if not os.environ.get("DISPLAY"):
        raise RuntimeError("provide an isolated X11 DISPLAY, for example with xvfb-run")
    process = subprocess.Popen(["openbox", "--sm-disable"]) if start else None
    try:
        wait_for_window_manager(process)
        yield
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=float, default=180, help="command timeout in seconds")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command or not 0 < args.timeout < float("inf"):
        parser.error("provide a command after -- and a positive finite timeout")
    with window_manager():
        return subprocess.run(command, timeout=args.timeout, check=True).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"with-openbox: {error}", file=sys.stderr)
        sys.exit(1)
