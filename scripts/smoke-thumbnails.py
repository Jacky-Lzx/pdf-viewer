#!/usr/bin/env python3
"""Exercise virtualized PDF previews in a real desktop. Run with xvfb-run -a."""
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-thumbnails-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = (fixture["PAGES"] * 14)[:80]
        # Preserve xref offsets while removing chapters, keeping controls near the top.
        data = re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()), bytes(fixture["make_pdf"]()), count=1)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(data)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        application = None
        window = None
        with open(root / "application.log", "w+") as log:
            def launch():
                nonlocal window
                process = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                window = wait_for(keyboard.focus, "thumbnail reader window")
                return process

            def saved(predicate):
                try:
                    state = json.loads(config_path.read_text())
                    return predicate(state, state["recent"][0])
                except (OSError, ValueError, KeyError, IndexError):
                    return False

            def expect(page, description):
                wait_for(lambda: saved(lambda _, book: book["page"] == page), description)

            def preview_ready():
                # Require actual PDF ink, rather than a loading placeholder or
                # the selection fill. All probes belong to the same frame.
                height = keyboard.size(window)[1]
                with keyboard.frame((14, 64, 230, height - 100)) as frame:
                    bands = selection(frame)
                    if bands:
                        top, bottom = bands[-1]
                        for y in range(top + 8, bottom - 8):
                            for x in range(25, 225, 3):
                                red, green, blue = frame.pixel((x, y))
                                if green > red * 1.4 and green > blue * 1.1:
                                    return True
                return False

            def selection(frame):
                return frame.bands([16], lambda color: color[1] > color[0] + 8
                                   and color[1] > color[2] + 6, min_height=12)

            def selected_bottom(height):
                with keyboard.frame((14, 64, 230, height - 100)) as frame:
                    bands = selection(frame)
                    return bands[-1][1] - 1 if bands else None

            def preview_point(relation="selected", night=False):
                height = keyboard.size(window)[1]
                with keyboard.frame((14, 64, 230, height - 100)) as frame:
                    first, end = 64, height - 36
                    if relation != "any":
                        bands = selection(frame)
                        if not bands:
                            return None
                        top, bottom = bands[-1]
                        if relation == "previous":
                            end = top
                        elif relation == "next":
                            first = bottom
                        else:
                            first, end = top, bottom
                    paper = (4, 5, 9) if night else (251, 250, 246)
                    for y in sorted(range(first, end, 2), key=lambda y: abs(y - (first + end) / 2)):
                        for x in sorted(range(40, 220, 3), key=lambda x: abs(x - 128)):
                            if frame.pixel((x, y)) == paper:
                                return x, y
                return None

            def paper_height(night=False):
                height = keyboard.size(window)[1]
                with keyboard.frame((14, 64, 230, height - 100)) as frame:
                    bands = selection(frame)
                    if bands:
                        top, bottom = bands[-1]
                        paper = (4, 5, 9) if night else (251, 250, 246)
                        rows = [y for y in range(top, bottom) if frame.pixel((128, y)) == paper]
                        if rows:
                            return rows[-1] - rows[0] + 1
                return None

            def compact_rows():
                # Read caption baselines and identify the selected row by its
                # fill. Positions follow actual font metrics and scrolling.
                with keyboard.frame((14, 64, 230, 720)) as frame:
                    bands = selection(frame)
                    if not bands:
                        return None
                    top, bottom = bands[-1]
                    rows, start = [], None
                    for y in range(64, 785):
                        ink = y < 784 and any(
                            max(color) < 160
                            for color in (frame.pixel((x, y)) for x in range(50, 220, 3)))
                        if ink and start is None:
                            start = y
                        elif not ink and start is not None:
                            if y - start >= 5:
                                rows.append((90, (start + y - 1) // 2))
                            start = None
                    selected = [index for index, (_, y) in enumerate(rows) if top <= y < bottom]
                    if len(selected) == 1:
                        return rows, selected[0]
                return None

            def next_compact_row():
                layout = compact_rows()
                if layout:
                    rows, index = layout
                    if index + 1 < len(rows):
                        return rows[index + 1]
                return None

            def final_compact_rows():
                layout = compact_rows()
                if layout:
                    rows, index = layout
                    if index >= 8:
                        return rows[index - 8:index + 1]
                return None

            try:
                application = launch()
                expect(0, "initial reading state")
                preferences_path = config_path.with_name("config.toml")
                preferences = preferences_path.read_text()
                keyboard.key("t")
                wait_for(lambda: saved(lambda state, _: state["thumbnails"]), "preview preference")
                wait_for(preview_ready, "actual PDF pixels in first preview")
                keyboard.click(wait_for(lambda: preview_point("next"), "rendered second preview"))
                expect(1, "click second preview")
                keyboard.key("g", shift=True)
                expect(79, "Vim G reaches final page")
                keyboard.click(wait_for(lambda: preview_point("previous"), "preceding preview after G"))
                expect(78, "preview list follows G to the final two pages")
                keyboard.key("g"); keyboard.key("g")
                expect(0, "Vim gg returns to first page")
                wait_for(preview_ready, "preview list follows gg")
                keyboard.wheel((90, 390), down=True, steps=9)
                wait_for(lambda: selected_bottom(820) is None, "manual scrolling hides the first preview")
                keyboard.click(wait_for(lambda: preview_point("any"), "rendered preview after scrolling"))
                wait_for(lambda: saved(lambda _, book: 1 < book["page"] < 20), "scroll and click a later preview")
                keyboard.key("g"); keyboard.key("g")
                wait_for(preview_ready, "return to cached first preview")
                upright = wait_for(paper_height, "upright PDF image bounds")
                keyboard.key("i")
                wait_for(lambda: preview_point(night=True), "inverted preview pixels")
                keyboard.key("r")
                wait_for(lambda: saved(lambda _, book: book["rotation"] == 1), "rotated reading view")
                wait_for(lambda: (paper_height(night=True) or upright) < upright,
                         "rotated preview has a shorter landscape image")
                keyboard.key("r", shift=True); keyboard.key("i")
                wait_for(preview_ready, "restore upright preview colors")
                keyboard.resize_reader(window, 860, 540)
                keyboard.key("g", shift=True)
                expect(79, "last page in minimum window")
                # The padded header leaves room for one full preview here.
                # Scroll slightly to expose the preceding page before clicking it.
                keyboard.wheel((90, 350), steps=2)
                keyboard.click(wait_for(lambda: preview_point("previous"), "preceding preview in minimum window"))
                expect(78, "preceding preview remains clickable in minimum window")
                # Expose the next card after focus has revealed the preceding
                # one, then click rendered paper below the selected card.
                keyboard.wheel((90, 350), down=True, steps=2)
                keyboard.click(wait_for(lambda: preview_point("next"), "visible last preview below the selected card"))
                expect(79, "last preview remains clickable in minimum window")
                application.terminate(); application.wait(timeout=5)
                assert preferences_path.read_text() == preferences, "temporary appearance changes must not rewrite TOML"
                application = launch()
                expect(79, "restore reading page")
                wait_for(lambda: saved(lambda state, _: not state["thumbnails"] and not state["sidebar"]), "restart reapplies configured appearance defaults")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                preferences_path.write_text("[appearance]\nsidebar = true\nthumbnails = true\n")
                application = launch()
                expect(79, "configuration edits preserve the reading page")
                wait_for(lambda: saved(lambda state, _: state["thumbnails"] and state["sidebar"]), "restart applies configured preview mode and sidebar")
                keyboard.key("g"); keyboard.key("g")
                wait_for(preview_ready, "restored preview mode renders PDF images")
                keyboard.key("t")
                wait_for(lambda: saved(lambda state, _: not state["thumbnails"]), "switch back to compact page list")
                keyboard.resize_reader(window, 1180, 820)
                wait_for(lambda: selected_bottom(820), "selected compact row after resizing")
                keyboard.wheel((90, 390), down=True, steps=9)
                wait_for(lambda: selected_bottom(820) is None, "manual scrolling hides the first compact row")
                keyboard.park_pointer()
                keyboard.key("g"); keyboard.key("g")
                keyboard.click(wait_for(next_compact_row, "gg reveals the first two compact page captions"))
                expect(1, "gg reveals current page after manually scrolling the compact list")
                keyboard.park_pointer()
                keyboard.key("g", shift=True)
                expect(79, "last page in compact list mode")
                rows = wait_for(final_compact_rows, "final nine compact page captions are drawn")
                keyboard.click(rows[0])
                expect(71, "compact list follows keyboard navigation to final pages")
                keyboard.click(rows[1])
                expect(72, "clicking a list row preserves its neighboring rows")
                keyboard.key(0xFF09)
                wait_for(lambda: saved(lambda state, _: not state["sidebar"]), "close sidebar")
                keyboard.key("t")
                wait_for(lambda: saved(lambda state, _: state["thumbnails"] and state["sidebar"]), "preview key reopens hidden sidebar")
                print("PASS: 80-page previews and compact list, rendered pixels, stable clicks, Vim tracking, same-page reveal, scrolling, inversion, rotation, compact window, reading progress and TOML appearance defaults")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application and application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
