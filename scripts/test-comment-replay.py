#!/usr/bin/env python3
"""Exercise the Actions-built, offline comment replay CLI without an app."""
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest


if len(sys.argv) != 2:
    raise SystemExit("Usage: test-comment-replay.py ACTIONS_BUILT_REPLAY_BINARY")
BINARY = Path(sys.argv.pop()).resolve()
MAGIC = b"HRNGCM01"
SOURCE_MARKERS = ("private-body", "private-cursor", "t3_fixture", "t1_fixturea", "www.reddit.com")
URL = "https://www.reddit.com/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&startingDepth=1&seeker-session=true&commentParentPositions=25"
HTML = """<shreddit-comment thingid="t1_fixturea" parentid="t3_fixture" depth="0" postid="t3_fixture">
<div slot="comment">private-body</div>
<shreddit-comment thingid="t1_fixtureb" parentid="t1_fixturea" depth="1" postid="t3_fixture"><div slot="comment">private-body child</div></shreddit-comment>
<faceplate-partial src="/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&amp;startingDepth=2&amp;seeker-session=true&amp;commentParentPositions=25" data-cursor="private-cursor"></faceplate-partial>
</shreddit-comment>
<faceplate-partial src="/svc/shreddit/more-comments/AskReddit/t3_fixture?sort=CONFIDENCE&amp;startingDepth=1&amp;seeker-session=true&amp;commentParentPositions=25"></faceplate-partial>"""


class ReplayTests(unittest.TestCase):
    def invoke(self, raw):
        with tempfile.TemporaryDirectory(prefix="hrungnir-replay-check-") as directory:
            fixture = Path(directory) / "fixture.bin"
            fixture.write_bytes(raw)
            result = subprocess.run([str(BINARY), str(fixture)], capture_output=True, text=True, timeout=20)
        for marker in SOURCE_MARKERS:
            self.assertNotIn(marker, result.stdout + result.stderr, "Replay output exposed source values")
        return result, json.loads(result.stdout)

    def fixture(self, html=HTML, **overrides):
        metadata = {
            "status_code": 200, "content_type_kind": "html", "encoding": "identity",
            "url": URL, "post_id": "t3_fixture", "parent_id": "t3_fixture", "community": "AskReddit",
        }
        metadata.update(overrides)
        encoded = json.dumps(metadata).encode()
        return MAGIC + struct.pack("<I", len(encoded)) + encoded + html.encode()

    def test_help_is_headless(self):
        result = subprocess.run([str(BINARY), "--help"], capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0)
        self.assertIn("Offline parser replay", result.stdout)

    def test_actual_parser_counts_comments_and_continuation_scopes(self):
        result, value = self.invoke(self.fixture())
        self.assertEqual(result.returncode, 0)
        self.assertEqual(value["status"], "parsed")
        self.assertEqual(value["comment_count"], 2)
        self.assertEqual(value["root_comment_count"], 1)
        self.assertEqual(value["nested_comment_count"], 1)
        self.assertEqual(value["root_continuation_parent_count"], 1)
        self.assertEqual(value["nested_continuation_parent_count"], 1)

    def test_wrong_branch_is_rejected_without_exposing_source(self):
        result, value = self.invoke(self.fixture(parent_id="t1_unrelated"))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(value["status"], "rejected")
        self.assertEqual(value["kind"], "branch_mismatch")

    def test_invalid_header_is_bounded(self):
        result, value = self.invoke(b"not a private-body capture")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(value["kind"], "invalid_fixture_header")

    def test_impossible_metadata_length_is_rejected(self):
        result, value = self.invoke(MAGIC + struct.pack("<I", 0xFFFFFFFF))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(value["kind"], "invalid_fixture_metadata_length")

    def test_unknown_metadata_type_is_not_echoed(self):
        result, value = self.invoke(self.fixture(content_type_kind="private-cursor"))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(value["kind"], "invalid_fixture_content_type")


if __name__ == "__main__":
    unittest.main()
