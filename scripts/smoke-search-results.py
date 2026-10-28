#!/usr/bin/env python3
"""Check search-result visibility below long contents and bookmarks in X11."""
import ctypes
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
ENTER, ESCAPE, TAB = 0xFF0D, 0xFF1B, 0xFF09


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-search-results-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [
            (f"Chapter {page + 1:03}", "CHAPTER", [f"Needle {page + 1:03}"])
            for page in range(80)
        ]
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture["make_pdf"]())
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({
            "sidebar": True,
            "recent": [{"path": str(document), "bookmarks": list(range(60))}],
        }))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        return json.loads(config_path.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def resize(width, height):
                keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            def results():
                keyboard.key("f", ctrl=True)
                for _ in range(5):
                    keyboard.key(TAB)
                keyboard.key(ENTER)
                keyboard.key(ESCAPE)

            def selected_result(height):
                # Highlight pixels identify search rows even while bookmarks,
                # chapters and the page list also have selected backgrounds.
                for y in range(112, height - 42, 2):
                    red, green, blue = keyboard.pixel((230, y))
                    if green > red + 8 and green > blue + 6:
                        for x in range(24, 200, 3):
                            if keyboard.pixel((x, y)) == (250, 220, 125):
                                return x, y
                return None

            try:
                window = wait_for(keyboard.focus, "long-outline search window")
                expect(0, "initial page")
                resize(860, 540)
                keyboard.key("f", ctrl=True)
                for character in "Needle":
                    keyboard.key(character.lower(), shift=character.isupper(), delay=0.025)
                keyboard.key(ENTER)
                time.sleep(0.6)
                keyboard.key(ESCAPE)
                keyboard.key("n", shift=True)
                expect(79, "reverse search reaches the last of 80 hits")
                wait_for(lambda: selected_result(540), "last result visible below contents and bookmarks")
                for width, height in ((1180, 820), (860, 240), (860, 540)):
                    resize(width, height)
                    wait_for(lambda: selected_result(height), "selected result retained after resizing")
                position = selected_result(540)
                keyboard.wheel(position, steps=12)
                time.sleep(0.3)
                assert not selected_result(540), "manual list scrolling must remain in place"
                results()
                position = wait_for(lambda: selected_result(540), "Results restores the current row")
                keyboard.key("k", shift=True)
                expect(78, "navigate away without changing the search selection")
                keyboard.click(position)
                expect(79, "the revealed selected row has a working click target")
                keyboard.key("n", shift=True)
                expect(78, "previous search result")
                wait_for(lambda: selected_result(540), "adjacent search result remains visible")
                keyboard.key("c")
                keyboard.key("a", ctrl=True)
                for character in "001 chapter":
                    keyboard.key(character, delay=0.025)
                keyboard.key(ENTER)
                expect(0, "chapter focus still reveals the outline after search navigation")
                results()
                position = wait_for(lambda: selected_result(540), "Results returns from focused contents")
                keyboard.click(position)
                expect(78, "Results preserves the current hit while reading another chapter")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: 80 search results below long contents and 60 bookmarks, first/last navigation, resize visibility, manual scrolling, Results restoration, clickable rows and chapter focus")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
