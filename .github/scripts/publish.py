#!/usr/bin/env python3
"""Publish the draft release and update the rolling update-feed release."""

import json
import os
import re
import subprocess
import tempfile
from datetime import datetime, timezone
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


def run(
    *args: str, check: bool = True, capture: bool = False
) -> subprocess.CompletedProcess:
    print(f"+ {' '.join(args)}")
    return subprocess.run(args, check=check, text=True, capture_output=capture)


def release_asset_names(tag: str) -> list[str] | None:
    """Asset names of the release, or None when the release does not exist."""
    probe = run(
        "gh",
        "release",
        "view",
        tag,
        "--repo",
        REPO,
        "--json",
        "assets",
        "--jq",
        ".assets[].name",
        check=False,
        capture=True,
    )
    if probe.returncode != 0:
        return None
    return [line for line in probe.stdout.splitlines() if line]


def download_asset(tag: str, pattern: str, dest_dir: Path) -> Path | None:
    """Download one asset by pattern; None when it is not in the release."""
    result = run(
        "gh",
        "release",
        "download",
        tag,
        "--repo",
        REPO,
        "--pattern",
        pattern,
        "--dir",
        str(dest_dir),
        "--clobber",
        check=False,
    )
    if result.returncode != 0:
        return None
    path = dest_dir / pattern
    return path if path.exists() else None


def build_fresh_feed(
    version: str, notes: str, assets: list[str], tag: str, tmp: Path
) -> dict:
    """Build an update feed from scratch (first release / no latest.json).

    Platforms point at real installer assets of the release. The signature
    comes from the matching `.sig` asset when the build produced one; without
    the updater configured there is no signature yet, and the feed says so.
    When the release has no installer asset at all, the feed ships with no
    platforms instead of inventing URLs to files that do not exist.
    """
    feed: dict = {
        "version": version,
        "pub_date": datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "platforms": {},
    }
    if notes:
        feed["notes"] = notes
    installer = next(
        (name for name in assets if name.endswith("-setup.exe")),
        next((name for name in assets if name.endswith(".msi")), None),
    )
    if installer is None:
        print("WARNING: no installer asset in the release; feed has no platforms")
        return feed
    signature = ""
    sig_path = download_asset(tag, installer + ".sig", tmp)
    if sig_path is not None:
        signature = sig_path.read_text(encoding="utf-8").strip()
    if not signature:
        print(
            "WARNING: no .sig asset for "
            f"{installer}; feed ships without a signature until the "
            "updater is configured"
        )
    feed["platforms"]["windows-x86_64"] = {
        "signature": signature,
        "url": f"https://github.com/{REPO}/releases/download/{tag}/{installer}",
    }
    return feed


def main() -> None:
    notes = extract_notes(VERSION)
    print(f"Extracted notes for {VERSION}: {len(notes)} chars")

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        latest_path = tmp / "latest.json"
        feed_path = tmp / "update-feed.json"
        release_exists = False

        downloaded = run(
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
            check=False,
        )
        if downloaded.returncode == 0 and latest_path.exists():
            feed = json.loads(latest_path.read_text(encoding="utf-8"))
            release_exists = True
        else:
            print(
                f"latest.json not found in release {TAG}; building a fresh feed"
            )
            assets = release_asset_names(TAG)
            if assets is None:
                print(f"release {TAG} not found; building a minimal feed")
                feed = build_fresh_feed(VERSION, notes, [], TAG, tmp)
            else:
                release_exists = True
                feed = build_fresh_feed(VERSION, notes, assets, TAG, tmp)

        if notes:
            feed["notes"] = notes
        feed["pub_date"] = (
            datetime.now(timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z")
        )
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

        if not release_exists:
            print(f"No release {TAG} to publish; feed published on its own.")
            return
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
        # Best-effort: also attach update-feed.json to the versioned tag release (ReChat convention)
        run(
            "gh",
            "release",
            "upload",
            TAG,
            str(feed_path),
            "--repo",
            REPO,
            "--clobber",
            check=False,
        )


if __name__ == "__main__":
    main()
