#!/usr/bin/env python3
"""Coordinate native app checks with a minimum 30-minute launch interval."""
import argparse
import datetime
import fcntl
import os
from pathlib import Path
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--claim", action="store_true", help="record an allowed launch")
args = parser.parse_args()
state = Path(__file__).resolve().parent.parent / "var" / "native-reader-last-launch"
state.parent.mkdir(parents=True, exist_ok=True)
with state.open("a+", encoding="ascii") as record:
    fcntl.flock(record, fcntl.LOCK_EX)
    record.seek(0)
    value = record.read(128).strip()
    try:
        last_launch = int(value) if value else 0
    except ValueError:
        raise SystemExit("Native test launch record is invalid; inspect it before retrying.")
    now = int(time.time())
    next_launch = last_launch + 30 * 60
    if now < next_launch:
        when = datetime.datetime.fromtimestamp(next_launch, datetime.timezone.utc).isoformat()
        print(f"Native app tests paused until {when}; headless checks remain available.")
        raise SystemExit(75)
    if args.claim:
        record.seek(0)
        record.truncate()
        record.write(f"{now}\n")
        record.flush()
        os.fsync(record.fileno())
        print("Native test window claimed; the next launch is at least 30 minutes away.")
    else:
        print("Native test interval is available; run headless checks before claiming it.")
