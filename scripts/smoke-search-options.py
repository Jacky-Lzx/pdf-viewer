#!/usr/bin/env python3
"""Check search options, keyboard focus and result replacement in a native window."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-search-options-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [(f"Case {page + 1}", "SEARCH OPTIONS", [line]) for page, line in enumerate([
            "Rustacean rusting trust", "RUST", "rust", "Rust", "RUST rust", "Rustacean",
        ])]
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()), bytes(fixture["make_pdf"]()), count=1))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        return json.loads(config_path.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def first():
                keyboard.key("g"); keyboard.key("g")
                expect(0, "first page")

            def find(query="Rust", toggles=""):
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                for character in query:
                    keyboard.key(character.lower(), shift=character.isupper(), delay=0.025)
                for option in toggles:
                    keyboard.key(option, alt=True, delay=0.015)
                # Enter while the 180ms debounce is pending must retain the jump.
                keyboard.key(0xFF0D)
                time.sleep(0.5)
                keyboard.key(0xFF1B)

            try:
                window = wait_for(keyboard.focus, "search options reader window")
                expect(0, "initial page")
                find()
                def counted(count, reverse=False, symbol="n"):
                    for digit in str(count):
                        keyboard.key(digit, delay=0.025)
                    keyboard.key(symbol, shift=reverse)
                counted(3)
                expect(1, "3n skips three matches, including matches on the same page")
                keyboard.key("j", shift=True)
                expect(2, "search count is consumed before the next page command")
                counted(3, reverse=True)
                expect(0, "3N reverses three matches")
                counted(8)
                expect(5, "counted forward search reaches the last match")
                counted(10)
                expect(0, "counts larger than all matches wrap correctly")
                counted(4, symbol=0xFFC0)
                expect(2, "4F3 uses the same counted search")
                counted(4, reverse=True, symbol=0xFFC0)
                expect(0, "4Shift+F3 reverses the count")
                counted(9999999)
                keyboard.key("j", shift=True)
                expect(1, "large search count resolves directly and leaves no numeric prefix")
                keyboard.key("k", shift=True)
                expect(0, "return before reverse wrap")
                keyboard.key("n", shift=True)
                expect(5, "default insensitive substring includes Rustacean on final page")
                first(); find(toggles="w")
                expect(1, "whole words reject Rustacean, rusting and trust")
                keyboard.key("n", shift=True)
                expect(4, "whole-word reverse wrap excludes final-page substring")
                keyboard.key("n")
                expect(1, "whole-word next wraps to uppercase RUST")
                first(); find(toggles="c")
                expect(3, "case-sensitive whole word selects only exact Rust")
                keyboard.key("r", ctrl=True)
                time.sleep(0.7)
                keyboard.key("n")
                expect(3, "reload retains both options")
                first(); find("RUST")
                expect(1, "uppercase exact whole word")
                first(); find("rust")
                expect(2, "lowercase exact whole word")
                first(); find(toggles="w")
                expect(0, "case-sensitive substring matches Rustacean")
                keyboard.key("n", shift=True)
                expect(5, "case-sensitive substring wraps to last Rustacean")
                first(); find(toggles="wccccc")
                expect(1, "rapid toggles replace earlier requests with insensitive whole words")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                first(); find(toggles="c")
                expect(3, "both search options work in the minimum window")
                # Both option buttons remain visible and clickable at 860x540.
                first(); keyboard.key("f", ctrl=True)
                def selected(position):
                    red, green, blue = keyboard.pixel(position)
                    return green > red * 1.4 and green > blue * 1.1
                assert selected((272, 67)) and selected((315, 67)), "Search option buttons are not drawn"
                keyboard.click((285, 77))
                assert not selected((272, 67)), "Aa button did not toggle off"
                keyboard.key(0xFF0D); time.sleep(0.5); keyboard.key(0xFF1B)
                expect(1, "Aa button enables insensitive whole-word matching")
                first(); keyboard.key("f", ctrl=True)
                keyboard.click((335, 77))
                assert not selected((315, 67)), "Word button did not toggle off"
                keyboard.key(0xFF0D); time.sleep(0.5); keyboard.key(0xFF1B)
                keyboard.key("n", shift=True)
                expect(5, "Word button restores substring matching")
                first(); find("absent")
                keyboard.key("n")
                time.sleep(0.5)
                expect(0, "no-match query clears the previous result selection")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                keyboard.key(0xFF08)
                keyboard.key(0xFF0D); keyboard.key(0xFF1B)
                keyboard.key("n")
                time.sleep(0.3)
                expect(0, "empty query does not restore stale matches")
                first(); find(toggles="cw")
                expect(3, "both options enabled before saving keyboard preferences")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert json.loads(config_path.read_text())["search_options"] == {"match_case": True, "whole_word": True}
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "reader restored with saved search options")
                time.sleep(0.4)
                first(); find()
                expect(3, "restarted reader actually uses saved case-sensitive whole-word matching")
                first(); keyboard.key("f", ctrl=True)
                assert selected((272, 67)) and selected((315, 67)), "saved option buttons must be selected"
                keyboard.click((285, 77)); keyboard.click((335, 77))
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert json.loads(config_path.read_text())["search_options"] == {"match_case": False, "whole_word": False}
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "reader restored after option button changes")
                time.sleep(0.4)
                first(); find()
                expect(0, "mouse option changes restore insensitive substring behavior after restart")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert json.loads(config_path.read_text())["recent"][0]["page"] == 0
                print("PASS: saved keyboard/button preferences and actual search behavior across restarts, old-config defaults, case/whole-word combinations, focused shortcuts, rapid replacement, deferred Enter, wrap, reload, empty/no matches and compact buttons")
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
