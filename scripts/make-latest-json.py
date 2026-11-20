#!/usr/bin/env python3
"""Build `latest.json` for the Tauri 2 updater from signed bundles.

The signature field is the *contents* of the `.sig` file Tauri produced at build time.
This script never touches the private key: it only reads public artifacts.

Usage:
    scripts/make-latest-json.py --version 0.2.0 --repo v5note/heron --dir dist \
        --out dist/latest.json
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Optional
from urllib.parse import quote

# Updater payload suffixes, in the order Tauri publishes them.
PLATFORM_SUFFIXES = (
    (".app.tar.gz", "darwin"),
    (".nsis.zip", "windows-x86_64"),
    (".appimage.tar.gz", "linux-x86_64"),
)


def platform_for(name: str) -> Optional[str]:
    lower = name.lower()
    if lower.endswith(".sig"):
        return None
    for suffix, platform in PLATFORM_SUFFIXES:
        if lower.endswith(suffix):
            # macOS payloads carry the architecture in the file name.
            if platform == "darwin":
                if "aarch64" in lower or "arm64" in lower:
                    return "darwin-aarch64"
                if "x64" in lower or "x86_64" in lower:
                    return "darwin-x86_64"
                return None
            return platform
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--repo", required=True, help="owner/name on GitHub")
    parser.add_argument("--dir", required=True, help="directory holding signed payloads")
    parser.add_argument("--out", required=True)
    parser.add_argument("--notes", default="")
    parser.add_argument("--tag", default=None, help="release tag (default: v<version>)")
    args = parser.parse_args()

    tag = args.tag or f"v{args.version}"
    root = Path(args.dir)
    platforms: dict[str, dict[str, str]] = {}

    for artifact in sorted(root.iterdir()):
        if not artifact.is_file():
            continue
        platform = platform_for(artifact.name)
        if platform is None:
            continue
        signature = artifact.with_name(artifact.name + ".sig")
        if not signature.is_file():
            print(f"missing signature for {artifact.name}", file=sys.stderr)
            return 1
        platforms[platform] = {
            "signature": signature.read_text(encoding="utf-8").strip(),
            "url": (
                f"https://github.com/{args.repo}/releases/download/"
                f"{quote(tag)}/{quote(artifact.name)}"
            ),
        }

    if not platforms:
        print("no updater payloads found", file=sys.stderr)
        return 1

    manifest = {
        "version": args.version,
        "notes": args.notes,
        "pub_date": __import__("datetime").datetime.now(__import__("datetime").timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "platforms": platforms,
    }
    Path(args.out).write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {args.out}: {', '.join(sorted(platforms))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
