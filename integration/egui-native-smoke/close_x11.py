"""Exercise native close policies with real WM_DELETE_WINDOW input on X11."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import time


def run_case(binary: Path, scenario: str) -> None:
    process = subprocess.Popen([str(binary), "--scenario", scenario])
    try:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"{scenario}: application exited before the close-ready child")
            result = subprocess.run(
                ["xdotool", "search", "--all", "--onlyvisible", "--pid", str(process.pid),
                 "--name", "^Dockspace native close ready$"],
                capture_output=True, text=True, timeout=3,
            )
            if result.returncode not in (0, 1):
                result.check_returncode()
            windows = result.stdout.split()
            if len(windows) > 1:
                raise RuntimeError(f"{scenario}: expected one close-ready child, got {windows}")
            if windows:
                subprocess.run(
                    ["xdotool", "windowactivate", "--sync", windows[0]], check=True, timeout=3,
                )
                # Alt-F4 requests WM_DELETE_WINDOW; windowclose would destroy the resource.
                subprocess.run(
                    ["xdotool", "key", "--clearmodifiers", "alt+F4"], check=True, timeout=3,
                )
                if process.wait(timeout=35):
                    raise RuntimeError(f"{scenario}: native close smoke failed")
                return
            time.sleep(0.05)
        raise RuntimeError(f"{scenario}: timed out waiting for the close-ready child")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="built egui-native-smoke executable")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    for scenario in ("recover-close", "retain-close", "cancel-close"):
        run_case(binary, scenario)


if __name__ == "__main__":
    main()
