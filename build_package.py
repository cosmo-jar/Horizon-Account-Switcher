#!/usr/bin/env python3
"""Packages Horizon Account Switcher into a deterministic .hnmod bundle."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import zipfile

ROOT = Path(__file__).resolve().parent
MOD_ID = "account_switcher"
VERSION = "1.0.0"
GAME = "26.3"
FIXED_TIME = (2026, 1, 1, 0, 0, 0)

ALLOWED_MOD_EXT = {".rs", ".toml"}


def is_valid_bundle_file(rel_to_mixin: Path) -> bool:
    name = rel_to_mixin.name
    parts = rel_to_mixin.parts
    if "target" in parts or ".git" in parts:
        return False
    if parts[0] == "data":
        return True
    if rel_to_mixin.suffix in ALLOWED_MOD_EXT:
        return True
    if name in ("LICENSE", "README.md"):
        return True
    return False


def main() -> None:
    compat = json.loads((ROOT / "compatibility.json").read_text(encoding="utf-8"))
    native_manifest = {
        "schema": 1,
        "id": MOD_ID,
        "version": VERSION,
        "game": GAME,
        "horizon": compat["horizon"],
        "mixins": compat["mixins"],
    }
    if compat.get("hooks") is not None:
        native_manifest["hooks"] = compat["hooks"]

    dist_dir = ROOT / "dist"
    dist_dir.mkdir(parents=True, exist_ok=True)
    out_file = dist_dir / f"{MOD_ID}-{VERSION}.hnmod"

    mixin_dir = ROOT / "mixins" / MOD_ID
    files_to_pack: list[tuple[str, bytes]] = [
        (
            "horizon-native.json",
            (json.dumps(native_manifest, indent=2) + "\n").encode("utf-8"),
        ),
        (
            f"mixins/{MOD_ID}/LICENSE",
            (ROOT / "LICENSE").read_bytes(),
        ),
    ]

    for path in sorted(mixin_dir.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(mixin_dir)
        if not is_valid_bundle_file(rel):
            continue
        arcname = f"mixins/{MOD_ID}/" + rel.as_posix()
        files_to_pack.append((arcname, path.read_bytes()))

    files_to_pack.sort(key=lambda item: item[0])

    with zipfile.ZipFile(out_file, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for arcname, payload in files_to_pack:
            zi = zipfile.ZipInfo(arcname, date_time=FIXED_TIME)
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = 0o644 << 16
            zf.writestr(zi, payload)

    sha256 = hashlib.sha256(out_file.read_bytes()).hexdigest()
    icon_path = ROOT / "icon.png"
    icon_size = icon_path.stat().st_size if icon_path.is_file() else 0

    print(f"Packaged: {out_file}")
    print(f"Entries:  {len(files_to_pack)}")
    print(f"Size:     {out_file.stat().st_size} bytes")
    print(f"SHA-256:  {sha256}")
    print(f"Icon PNG: {icon_size} bytes (limit 262144)")


if __name__ == "__main__":
    main()
