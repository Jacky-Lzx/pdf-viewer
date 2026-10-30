#!/usr/bin/env python3
"""Exercise the shipped focus plugin's persistent protocol using only stdlib."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def event(number, kind, key=None, page=1, include_lines=False, scanned=False):
    context = {
        "document": "/tmp/中文 reading.pdf", "page": page, "page_count": 3,
        "page_size": [600, 800], "rotation": 0, "zoom": 1,
        "scroll": [0, 0], "viewport": [600, 800], "selected_text": "",
        "lines_truncated": False,
    }
    if include_lines:
        context["lines"] = [] if scanned else [
            {"text": f"Line {index}", "rect": [60, 100 + index * 20, 400, 115 + index * 20]}
            for index in range(8)
        ]
    request = {"api_version": 2, "id": number, "event": kind, "context": context}
    if key is not None:
        request["key"] = key
    return request


def main():
    events = [
        event(1, "activate", include_lines=True),
        event(2, "key", "j"),
        event(3, "key", "k"),
        event(4, "view"),
        event(5, "page", page=2, include_lines=True),
        event(6, "key", "k", page=2),
        event(7, "page", include_lines=True),
        event(8, "key", "j"),
        event(9, "page", page=2, include_lines=True, scanned=True),
        event(10, "key", "j", page=2),
    ]
    output = subprocess.run(
        [sys.executable, "-u", str(ROOT / "examples/plugins/focus-reader/plugin.py")],
        input="".join(json.dumps(request) + "\n" for request in events),
        text=True, capture_output=True, check=True, timeout=5,
    )
    replies = [json.loads(line) for line in output.stdout.splitlines()]
    assert [reply["id"] for reply in replies] == list(range(1, 11))
    hole = lambda reply: reply["scene"]["pages"][0]["holes"][0]
    assert hole(replies[1])[1] > hole(replies[0])[1]
    assert hole(replies[2]) == hole(replies[0])
    assert hole(replies[3]) == hole(replies[2]) and "reveal" not in replies[3]
    assert replies[5]["goto_page"] == 1
    assert hole(replies[6])[1] > hole(replies[0])[1], "backward page entry starts at bottom"
    assert replies[7]["goto_page"] == 2
    assert hole(replies[9])[1] > hole(replies[8])[1], "scanned-page band moves"
    configured = event(1, "activate", include_lines=True)
    configured["context"]["settings"] = {"lines": 1, "opacity": 0.2}
    output = subprocess.run(
        [sys.executable, "-u", str(ROOT / "examples/plugins/focus-reader/plugin.py")],
        input=json.dumps(configured) + "\n", text=True, capture_output=True, check=True, timeout=5,
    )
    reply = json.loads(output.stdout)
    assert reply["scene"]["dim"][3] == 51
    assert hole(reply)[3] - hole(reply)[1] < hole(replies[0])[3] - hole(replies[0])[1]
    print("PASS: focus session, j/k, view stability, forward/backward page boundaries and scanned fallback")


if __name__ == "__main__":
    main()
