"""Run the pinned fork-backed native workspace tests."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

def run(command: list[str], cwd: Path) -> None:
    subprocess.run(command, cwd=cwd, check=True)


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
            "--workspace",
            "--all-targets",
            "--no-fail-fast",
            "--locked",
            "-j1",
        ],
        repo_root,
    )
    return 0


def entrypoint() -> int:
    try:
        return main()
    except subprocess.CalledProcessError as error:
        print(
            f"fork-backed native tests failed with exit code {error.returncode}",
            file=sys.stderr,
        )
        return error.returncode


if __name__ == "__main__":
    sys.exit(entrypoint())
