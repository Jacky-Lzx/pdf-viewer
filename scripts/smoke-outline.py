#!/usr/bin/env python3
"""Exercise nested PDF outlines in a real desktop. Run with xvfb-run -a."""
import ctypes
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
make_pdf = runpy.run_path(str(SCRIPTS / "make-sample.py"))["make_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-outline-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        document.write_bytes(make_pdf(nested_outline=True))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(page, description):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return book["page"] == page and book["rotation"] == 0
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def filter_chapters(query):
                keyboard.click((90, 171))
                keyboard.key("a", ctrl=True)
                for letter in query:
                    keyboard.key(letter)
                time.sleep(0.2)

            def burst(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    for symbol, modifiers in keys:
                        keyboard.key(symbol, delay=0, **modifiers)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def chapter_keys(query):
                return [("c", {}), ("a", {"ctrl": True})] + [(letter, {}) for letter in query]

            try:
                window = wait_for(keyboard.focus, "outline reader window")
                expect(0, "initial reading state")
                keyboard.key(0xFF09)
                time.sleep(0.3)
                keyboard.click((100, 272))
                expect(2, "third-level named destination")
                keyboard.click((30, 235))
                keyboard.click((100, 272))
                expect(3, "folded chapter hides its child and keeps its next sibling")
                keyboard.click((18, 200))
                keyboard.click((90, 237))
                expect(4, "folding a root preserves other root chapters")
                filter_chapters("yourself")
                expect(4, "chapter typing does not trigger navigation or rotation")
                keyboard.key(0xFF1B)
                keyboard.click((100, 272))
                expect(2, "filter reveals matching title and both folded ancestors")
                keyboard.click((201, 171))
                keyboard.click((90, 237))
                expect(4, "clearing a filter restores the previous root fold")
                keyboard.click((166, 145))
                keyboard.click((100, 272))
                expect(2, "expand all restores nested chapters")
                keyboard.click((201, 145))
                keyboard.click((90, 237))
                expect(4, "collapse all keeps both root chapters")
                filter_chapters("no-such-chapter")
                keyboard.key(0xFF1B)
                keyboard.click((201, 171))
                filter_chapters("yourself")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key(0xFF1B)
                keyboard.click((100, 272))
                expect(2, "filtered chapter remains usable in the smallest window")
                keyboard.key("j", shift=True)
                expect(3, "Vim navigation resumes after leaving chapter filter")
                burst(chapter_keys("yourself") + [(0xFF0D, {})])
                expect(2, "opening-frame query and Enter follow title match instead of its ancestors")
                keyboard.key("j", shift=True)
                expect(3, "chapter confirmation returns focus to the reader")
                keyboard.key(0xFF09)
                burst(chapter_keys("yourself") + [("k", {"ctrl": True}), (0xFF0D, {})])
                expect(1, "c reopens a hidden sidebar and Ctrl+K selects a retained ancestor")
                burst(chapter_keys("yourself") + [(0xFF52, {}), ("j", {"ctrl": True}), (0xFF0D, {})])
                expect(2, "arrow and Ctrl+J navigation apply after the new filter")
                burst(chapter_keys("jkgdic") + [(0xFF0D, {})])
                keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "jkgdic", "opening c must preserve later literal c"
                expect(2, "unmatched chapter input does not navigate, rotate or change fit")
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(3, "Escape from an unmatched filter releases keyboard navigation")
                burst(chapter_keys("yourself") + [("f", {"ctrl": True}), ("a", {"ctrl": True})]
                      + [(letter, {}) for letter in "native"] + [(0xFF0D, {})])
                expect(0, "global search replaces pending chapter focus in the same burst")
                keyboard.key(0xFF1B)
                burst([("c", {}), (0xFF1B, {}), ("j", {"shift": True})])
                expect(1, "Escape cancels opening chapter focus before the next reader key")
                print("PASS: nested folds, Unicode-ready filtering, ancestor context, compact layout, same-frame chapter input/Enter, keyboard selection, hidden sidebar, no-match handling and focus switching")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.send_signal(signal.SIGCONT)
                application.terminate()
                application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
