"""Run the excluded fork-backed egui integration gate."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys


def fork_environment() -> dict[str, str]:
    environment = os.environ.copy()
    existing = environment.get("RUSTFLAGS", "").strip()
    bridge = "--cfg egui_backend_event_envelope"
    environment["RUSTFLAGS"] = f"{existing} {bridge}".strip()
    return environment


def run(command: list[str], cwd: Path) -> None:
    subprocess.run(
        command,
        cwd=cwd,
        check=True,
        env=fork_environment(),
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--check-only",
        action="store_true",
        help="Compile all targets without running nextest",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    manifest = repo_root / "integration" / "egui-fork-harness" / "Cargo.toml"
    native_manifest = repo_root / "crates" / "egui_dockspace_native" / "Cargo.toml"

    if args.check_only:
        run(
            [
                "cargo",
                "check",
                "--manifest-path",
                str(manifest),
                "--all-targets",
                "--locked",
            ],
            repo_root,
        )
        run(
            [
                "cargo",
                "check",
                "--manifest-path",
                str(native_manifest),
                "--all-targets",
                "--locked",
            ],
            repo_root,
        )
    else:
        run(
            [
                "cargo",
                "nextest",
                "run",
                "--manifest-path",
                str(manifest),
                "--all-targets",
                "--no-fail-fast",
                "--locked",
            ],
            repo_root,
        )
        run(
            [
                "cargo",
                "nextest",
                "run",
                "--manifest-path",
                str(native_manifest),
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
