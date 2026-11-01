#!/usr/bin/env python3
"""Reject interrupted or failed native reader journeys, including exit-zero ones."""
import json
from pathlib import Path
import sys

try:
    report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    cases = report["cases"]
    if (report.get("schema_version") != 1
            or report.get("result") != "completed"
            or report.get("probe") != "debug-native-wkwebview"
            or not isinstance(cases, list)
            or len(cases) != report.get("case_count")
            or not cases
            or report.get("failed_or_empty_cases") != 0):
        raise ValueError("Native reader report is incomplete or contains failed checks")
    if any(not isinstance(case, dict) or case.get("status") != "ok" for case in cases):
        raise ValueError("A native reader case did not pass")
    if not any(case.get("name") == "native-ui" for case in cases):
        raise ValueError("Native reader controls were not verified")
    if not any(str(case.get("name", "")).startswith("comments:") for case in cases):
        raise ValueError("Native discussion continuation was not verified")
except (IndexError, KeyError, OSError, ValueError, TypeError) as error:
    print(f"Native reader check failed: {error}", file=sys.stderr)
    raise SystemExit(1)
print(f"Native reader checks passed: {len(cases)} cases")
