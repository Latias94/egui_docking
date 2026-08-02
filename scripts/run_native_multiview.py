"""Run the fork-backed native multiview example from a pinned egui checkout."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

from run_egui_fork_harness import fork_environment


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    manifest = repo_root / "crates" / "egui_dockspace_native" / "Cargo.toml"
    command = [
        "cargo",
        "run",
        "--manifest-path",
        str(manifest),
        "--example",
        "native_multiview",
        "--locked",
    ]
    try:
        return subprocess.run(
            command,
            cwd=repo_root,
            check=False,
            env=fork_environment(),
        ).returncode
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
