#!/usr/bin/env python3
"""Publish the draft release and update the rolling update-feed release."""

import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

REPO = "thaarfknight-star/nexora"
FEED_TAG = "update-feed"
TAG = os.environ["TAG"]
VERSION = TAG.lstrip("v")
CHANGELOG = Path("CHANGELOG.md")
FA_DIGITS = str.maketrans("۰۱۲۳۴۵۶۷۸۹٫", "0123456789.")


def normalize_digits(text: str) -> str:
    return text.translate(FA_DIGITS)


def extract_notes(version: str) -> str:
    if not CHANGELOG.exists():
        return ""
    content = CHANGELOG.read_text(encoding="utf-8")
    pattern = re.compile(
        r"^##\s+\[?v?([^\s\]-]+)[^\n]*\n(.*?)(?=^##\s+|\Z)",
        re.MULTILINE | re.DOTALL,
    )
    target = normalize_digits(version)
    for match in pattern.finditer(content):
        heading_ver = normalize_digits(match.group(1))
        if heading_ver == target:
            return match.group(2).strip()
    return ""


def run(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    print(f"+ {' '.join(args)}")
    return subprocess.run(args, check=check, text=True)


def main() -> None:
    notes = extract_notes(VERSION)
    print(f"Extracted notes for {VERSION}: {len(notes)} chars")

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        latest_path = tmp / "latest.json"
        feed_path = tmp / "update-feed.json"

        run(
            "gh",
            "release",
            "download",
            TAG,
            "--repo",
            REPO,
            "--pattern",
            "latest.json",
            "--dir",
            str(tmp),
            "--clobber",
        )

        feed = json.loads(latest_path.read_text(encoding="utf-8"))
        if notes:
            feed["notes"] = notes
        feed_path.write_text(
            json.dumps(feed, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )

        probe = run("gh", "release", "view", FEED_TAG, "--repo", REPO, check=False)
        if probe.returncode == 0:
            run(
                "gh",
                "release",
                "upload",
                FEED_TAG,
                str(feed_path),
                "--repo",
                REPO,
                "--clobber",
            )
        else:
            run(
                "gh",
                "release",
                "create",
                FEED_TAG,
                str(feed_path),
                "--repo",
                REPO,
                "--title",
                "Update Feed",
                "--notes",
                "Rolling update feed for Nexora.",
            )

        edit_cmd = [
            "gh",
            "release",
            "edit",
            TAG,
            "--repo",
            REPO,
            "--draft=false",
        ]
        if notes:
            notes_file = tmp / "notes.md"
            notes_file.write_text(notes, encoding="utf-8")
            edit_cmd.extend(["--notes-file", str(notes_file)])
        run(*edit_cmd)


if __name__ == "__main__":
    main()
