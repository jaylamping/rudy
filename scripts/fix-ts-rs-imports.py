#!/usr/bin/env python3
"""Normalize ts-rs imports after `cargo test -p rudydae export_bindings`.

ts-rs emits imports to `crates/rudydae/bindings/serde_json/JsonValue` for serde_json::Value;
we keep JsonValue next to the other bindings under `link/src/lib/types/serde_json/`.
"""
from __future__ import annotations

from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TYPES = REPO / "link" / "src" / "lib" / "types"

REPLACEMENTS: tuple[tuple[str, str], ...] = (
    (
        'from "../../../../crates/rudydae/bindings/serde_json/JsonValue"',
        'from "./serde_json/JsonValue"',
    ),
)


def main() -> None:
    for ts_file in sorted(TYPES.glob("*.ts")):
        text = ts_file.read_text(encoding="utf-8")
        new = text
        for old, new_s in REPLACEMENTS:
            new = new.replace(old, new_s)
        if new != text:
            ts_file.write_text(new, encoding="utf-8")
            print("fixed", ts_file.relative_to(REPO))


if __name__ == "__main__":
    main()
