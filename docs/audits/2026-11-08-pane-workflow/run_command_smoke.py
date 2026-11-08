import json, os, subprocess, sys, time
manager = subprocess.Popen(["openbox"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    result = subprocess.run(["/native-target/debug/examples/basic", "--command-smoke"], capture_output=True, text=True, timeout=75)
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    if result.returncode or "command-smoke: complete;" not in result.stderr:
        raise SystemExit(1)
finally:
    manager.terminate()
    manager.wait(timeout=5)
