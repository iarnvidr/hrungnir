#!/usr/bin/env python3
"""Export browser evidence without losing the runner's failure status."""

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", type=Path, help="fail unless this runner result passed")
    args = parser.parse_args()
    if args.verify is not None:
        result = json.loads(args.verify.read_text())
        code = result.get("exit_code")
        if type(code) is not int or code != 0:
            raise SystemExit(f"Reader interaction checks failed (exit {code}); inspect the reader UX artifact.")
        print("Reader interaction checks passed.")
        return

    report = ROOT / "artifacts" / "reader-ux"
    report.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [sys.executable, "tests/reader-ux/run.py"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=False,
    )
    (report / "runner.log").write_text(result.stdout)
    (report / "result.json").write_text(json.dumps({"exit_code": result.returncode}) + "\n")
    print(result.stdout, end="")


if __name__ == "__main__":
    main()
