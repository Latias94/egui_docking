"""Run the real two-window fork-backed native smoke gate."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys

from run_egui_fork_harness import fork_environment


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--timeout",
        type=int,
        default=300,
        help="Maximum build and process duration in seconds",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    manifest = repo_root / "integration" / "egui-fork-workspace" / "Cargo.toml"
    command = [
        "cargo",
        "run",
        "--manifest-path",
        str(manifest),
        "--package",
        "egui-dockspace-native-e2e",
        "--locked",
    ]
    try:
        return subprocess.run(
            command,
            cwd=repo_root,
            check=False,
            timeout=args.timeout,
            env=fork_environment(repo_root),
        ).returncode
    except subprocess.TimeoutExpired:
        print(f"native E2E timed out after {args.timeout} seconds", file=sys.stderr)
        return 124
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
