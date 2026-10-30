"""Persistent focus overlay. No PDF rewriting or third-party dependencies."""
import argparse
import json
import sys

parser = argparse.ArgumentParser()
parser.add_argument("--lines", type=int, default=3)
parser.add_argument("--opacity", type=float, default=0.72)
options = parser.parse_args()
if not 1 <= options.lines <= 20 or not 0 <= options.opacity <= 1:
    parser.error("lines must be 1–20 and opacity must be 0–1")

lines = []
index = 0
enter_from = None

for raw in sys.stdin:
    request = json.loads(raw)
    if request["api_version"] != 2:
        raise SystemExit("Unsupported Pagewing API")
    context = request["context"]
    settings = context.get("settings", {})
    visible_lines = settings.get("lines", options.lines)
    opacity = settings.get("opacity", options.opacity)
    width, height = context["page_size"]
    event = request["event"]
    if "lines" in context:
        lines = context["lines"]
        # Scanned/image-only pages use a movable band instead of requiring OCR.
        if not lines:
            lines = [{"rect": [0, n * height / 60, width, (n + 1) * height / 60]}
                     for n in range(60)]
        index = max(0, len(lines) - visible_lines) if enter_from == "previous" else 0
        enter_from = None
    last = max(0, len(lines) - visible_lines)
    index = min(index, last)
    reply = {"id": request["id"], "capture_keys": ["j", "k", "ArrowDown", "ArrowUp"]}
    if event == "key":
        direction = 1 if request["key"] in ("j", "ArrowDown") else -1
        following = index + direction
        target_page = context["page"] + direction
        if (following < 0 or following > last) and 1 <= target_page <= context["page_count"]:
            enter_from = "previous" if direction < 0 else "next"
            reply["goto_page"] = target_page
            print(json.dumps(reply), flush=True)
            continue
        index = max(0, min(last, following))
    if lines:
        selected = lines[index:index + visible_lines]
        top = max(0, min(line["rect"][1] for line in selected) - 3)
        bottom = min(height, max(line["rect"][3] for line in selected) + 3)
        band = [0, top, width, bottom]
        reply["scene"] = {
            "dim": [0, 0, 0, round(opacity * 255)],
            "pages": [{"page": context["page"], "holes": [band]}],
        }
        if event in ("activate", "page", "key"):
            reply["reveal"] = band
    print(json.dumps(reply), flush=True)
