#!/usr/bin/env python3
"""Check ordered phrase search across PDF line breaks in a native X11 window."""
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
ENTER, ESCAPE = 0xFF0D, 0xFF1B


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-search-phrases-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [
            (f"Phrase {index}", "PHRASE SEARCH", lines)
            for index, lines in enumerate([
                ["xalpha extraordinary omegax"],
                ["alpha", "extraordinary", "omega"],
                ["ALPHA   EXTRAORDINARY  OMEGA"],
                ["alpha extraordinary omega", "alpha extraordinary omega"],
                ["alphaextraordinaryomega"],
                ["omega extraordinary alpha"],
            ])
        ]
        document = root / "pagewing-smoke.pdf"
        original = bytes(fixture["make_pdf"]())
        document.write_bytes(original)
        extracted = subprocess.run([str(binary), "--text", str(document), "2"],
                                   check=True, capture_output=True).stdout.decode()
        assert "alpha\nextraordinary\nomega" in extracted, extracted
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        state = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        return json.loads(state.read_text())["recent"][0]["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def find(query="alpha extraordinary omega", options=""):
                keyboard.key("g"); keyboard.key("g")
                expect(0, "return to first page")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                for character in query:
                    keyboard.key(character, delay=0.015)
                for option in options:
                    keyboard.key(option, alt=True)
                keyboard.key(ENTER)
                time.sleep(0.5)
                keyboard.key(ESCAPE)

            try:
                window = wait_for(keyboard.focus, "phrase-search reader window")
                expect(0, "initial page")
                find()
                keyboard.key("n")
                expect(1, "space-separated phrase finds text split over three PDF lines")
                keyboard.key("n")
                expect(2, "case folding and repeated spaces")
                keyboard.key("n")
                expect(3, "ordinary single-line phrase")
                keyboard.key("n")
                expect(3, "second phrase on the same page")
                keyboard.key("n")
                expect(0, "wrap excludes concatenated words and reversed order")
                find(options="w")
                expect(1, "whole-word phrase rejects attached prefix and suffix")
                keyboard.key("n", shift=True)
                expect(3, "reverse wrap with whole-word phrases")
                find(options="c")
                expect(1, "case-sensitive whole-word cross-line phrase")
                keyboard.key("n")
                expect(3, "case-sensitive phrase skips uppercase text")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                                       ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                find()
                expect(1, "cross-line phrase at minimum window size")
                keyboard.key("r", ctrl=True)
                time.sleep(0.7)
                keyboard.key("n")
                expect(1, "reload rebuilds phrase search without losing options")
                keyboard.key("n")
                expect(3, "next phrase after reload")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert document.read_bytes() == original
                print("PASS: three-line phrases, repeated spaces, case folding, whole words, ordered non-overlapping results, reverse wrap, minimum window, reload and unchanged PDF")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if state.exists():
                    print(state.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
