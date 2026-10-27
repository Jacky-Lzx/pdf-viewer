#!/usr/bin/env python3
"""Exercise a large bookmark list, visible selections, edits and shorter reloads."""
import ctypes
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-bookmarks-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        document = root / "pagewing-smoke.pdf"

        def write_document(count):
            fixture["PAGES"][:] = [(f"Page {page + 1}", "BOOKMARKS", ["Large bookmark list."]) for page in range(count)]
            # Keep xref offsets unchanged when removing the outline UI.
            data = re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()), bytes(fixture["make_pdf"]()), count=1)
            document.write_bytes(data)

        write_document(600)
        second_document = root / "pagewing-second.pdf"
        second_document.write_bytes(document.read_bytes())
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"sidebar": True, "recent": [
            {"path": str(document), "bookmarks": list(range(600))},
            {"path": str(second_document), "page": 5, "bookmarks": [5, 10]},
        ]}))
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description, removed=None, count=600):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        marks = book["bookmarks"]
                        return (book["page"] == page and len(marks) == count
                                and (removed is None or removed not in marks))
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def selected_bookmark_y():
                positions = []
                for y in range(160, 302, 2):
                    colors = [keyboard.pixel((x, y)) for x in (20, 80, 150)]
                    if all(green > red * 1.4 and green > blue * 1.1 for red, green, blue in colors):
                        positions.append(y)
                if len(positions) >= 6:
                    return (min(positions) + max(positions)) // 2
                return None

            try:
                window = wait_for(keyboard.focus, "bookmark reader window")
                expect(0, "initial reading state with 600 bookmarks")
                keyboard.key("m", ctrl=True, shift=True)  # Use the full reading canvas.
                wait_for(selected_bookmark_y, "first bookmark is visible")
                keyboard.key("g", shift=True)
                expect(599, "G reaches the final bookmarked page")
                last_y = wait_for(selected_bookmark_y, "last of 600 bookmarks is actually drawn")
                keyboard.click((100, last_y - 35))
                expect(598, "clicking the preceding virtual row follows its real bookmark")
                keyboard.key("b")
                expect(598, "removing a bookmark refreshes the visible index", removed=598, count=599)
                keyboard.key("b")
                expect(598, "restoring a bookmark updates the ordered index", count=600)
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key("g"); keyboard.key("g")
                expect(0, "gg reaches the first page in the compact window")
                wait_for(selected_bookmark_y, "first bookmark is visible after compact scrolling")
                keyboard.key("g", shift=True)
                expect(599, "last page remains reachable in the compact window")
                wait_for(selected_bookmark_y, "last bookmark remains visible in the compact window")
                write_document(30)
                keyboard.key("r", ctrl=True)
                expect(29, "shorter reload clamps both page and bookmark index", count=30)
                wait_for(selected_bookmark_y, "clamped last bookmark is visible after reload")
                keyboard.key("g"); keyboard.key("g")
                expect(0, "return to the first retained bookmark", count=30)
                keyboard.key("b")
                expect(0, "remove first retained bookmark", removed=0, count=29)
                keyboard.click((100, 171))
                expect(1, "first virtual row no longer points at a removed bookmark", removed=0, count=29)
                keyboard.key("p", ctrl=True)
                for letter in "pagewing-second":
                    keyboard.key(letter, delay=0.01)
                keyboard.key(0xFF0D)
                expect(5, "switching documents replaces the bookmark snapshot", count=2)
                keyboard.click((100, 206))
                expect(10, "the new document's second bookmark has the correct destination", count=2)
                keyboard.key("j", alt=True)
                expect(5, "Alt+J wraps to the first bookmark", count=2)
                keyboard.key("k", alt=True)
                expect(10, "Alt+K wraps to the last bookmark", count=2)
                keyboard.key("7"); keyboard.key("j", alt=True)
                expect(5, "counted bookmark navigation wraps without visiting intermediate pages", count=2)
                for _ in range(7):
                    keyboard.key("9", delay=0.01)
                keyboard.key("k", alt=True)
                expect(10, "large reverse count resolves in one jump", count=2)
                keyboard.key("/")
                for letter in "jkgdi":
                    keyboard.key(letter, delay=0.01)
                keyboard.key("j", alt=True); keyboard.key("k", alt=True)
                expect(10, "bookmark shortcuts do not navigate while typing a query", count=2)
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(11, "bookmark count is consumed before ordinary reading navigation", count=2)
                keyboard.key(0xFF51, alt=True)
                expect(10, "history returns from ordinary reading navigation", count=2)
                keyboard.key(0xFF51, alt=True)
                expect(5, "a large counted bookmark jump records one history entry", count=2)
                keyboard.key(0xFF53, alt=True)
                expect(10, "history can revisit the counted bookmark destination", count=2)
                keyboard.key("k", alt=True); keyboard.key("b")
                expect(5, "remove the first of two bookmarks", removed=5, count=1)
                keyboard.key("j", alt=True); keyboard.key("b")
                expect(10, "remove the last bookmark", removed=10, count=0)
                keyboard.key("j", alt=True); keyboard.key("k", alt=True)
                expect(10, "an empty bookmark list leaves the page unchanged", count=0)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 600 bookmarks, visible rows, clicks, add/remove, compact layout, reload/switch, counted keyboard jumps, wrapping, input focus, history and empty lists")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
