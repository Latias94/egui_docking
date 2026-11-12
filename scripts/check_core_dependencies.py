"""Reject UI/platform frameworks in the core consumer and build dependency graph."""

from pathlib import Path
import subprocess
import sys


def main() -> int:
    result = subprocess.run(
        [
            "cargo", "tree", "--package", "dockspace", "--all-features",
            "--target", "all", "--edges", "normal,build", "--prefix", "none",
            "--format", "{p}", "--locked",
        ],
        cwd=Path(__file__).resolve().parent.parent,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    print(result.stdout, end="")
    packages = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
    frameworks = ("egui", "eframe", "emath", "epaint", "accesskit", "winit", "wgpu", "gpui", "open-gpui")
    leaked = sorted(
        package for package in packages
        if any(
            package == framework or package.startswith((framework + "-", framework + "_"))
            for framework in frameworks
        )
    )
    if leaked:
        print("Core depends on UI/platform frameworks: " + ", ".join(leaked), file=sys.stderr)
        return 1
    print("Core dependency boundary passed (normal/build, all features and targets).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
