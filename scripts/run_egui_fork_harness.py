"""Run the excluded fork-backed egui integration gate."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys


def fork_environment(repo_root: Path) -> dict[str, str]:
    environment = os.environ.copy()
    existing = environment.get("RUSTFLAGS", "").strip()
    bridge = "--cfg egui_backend_event_envelope"
    environment["RUSTFLAGS"] = f"{existing} {bridge}".strip()
    environment["CARGO_TARGET_DIR"] = str(repo_root / "target")
    return environment


def run(command: list[str], cwd: Path) -> None:
    subprocess.run(
        command,
        cwd=cwd,
        check=True,
        env=fork_environment(cwd),
    )


def main() -> int:
    repo_root = Path(__file__).resolve().parents[1]
    manifest = repo_root / "integration" / "egui-fork-workspace" / "Cargo.toml"
    run(
        [
            "cargo",
            "nextest",
            "run",
            "--manifest-path",
            str(manifest),
            "--package",
            "egui-dockspace-fork-harness",
            "--package",
            "egui_dockspace_native",
            "--all-targets",
            "--no-fail-fast",
            "--locked",
        ],
        repo_root,
    )
    return 0


def entrypoint() -> int:
    try:
        return main()
    except subprocess.CalledProcessError as error:
        print(
            f"egui fork harness command failed with exit code {error.returncode}",
            file=sys.stderr,
        )
        return error.returncode


if __name__ == "__main__":
    sys.exit(entrypoint())
