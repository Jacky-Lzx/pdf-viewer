#!/usr/bin/env python3
"""Check native search-history recall, focus, clearing and window lifetime."""
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


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-search-history-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        original = (SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes()
        document.write_bytes(original)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def book():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, description):
                wait_for(lambda: book().get("page") == page, description)

            def text(value):
                for char in value:
                    keyboard.key(char.lower(), shift=char.isupper(), delay=0.025)

            def search(value):
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                text(value)
                keyboard.key(0xFF0D)

            def history(has_results):
                keyboard.key("f", ctrl=True)
                for _ in range(7 if has_results else 6):
                    keyboard.key(0xFF09)  # Aa, Word, Prev, Next, Results, optional Copy, History.
                keyboard.key(0xFF0D)

            def choose(index):
                for _ in range(index):
                    keyboard.key(0xFF54)
                keyboard.key(0xFF0D)

            def copied_query(select_all=False):
                if select_all:
                    keyboard.key("a", ctrl=True)
                keyboard.key("c", ctrl=True)
                return keyboard.clipboard()

            try:
                window = wait_for(keyboard.focus, "search history reader")
                expect(0, "initial reading page")
                search("Rust")
                expect(0, "first explicit query")
                search("Make")
                expect(2, "second explicit query")
                history(True)
                choose(1)  # Older Rust query, behind Make.
                assert copied_query() == "Rust", "recall returns focus with the entire query selected"
                assert book()["page"] == 2, "selecting history must not follow a match"
                keyboard.key(0xFF0D)
                expect(3, "Enter executes the recalled query with fresh results")
                search("jkgmissing")
                time.sleep(0.5)
                keyboard.key("w", ctrl=True)
                keyboard.key("t", ctrl=True, shift=True)
                wait_for(keyboard.focus, "reopened document in the same reader window")
                expect(3, "reopening retains the reading position")
                def reopened_text_ready():
                    keyboard.key("y")
                    return "Try searching for Rust." in keyboard.clipboard()
                wait_for(reopened_text_ready, "reopened document text is available")
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                history(False)
                choose(2)  # Make survives the document boundary, behind missing and Rust.
                assert copied_query() == "Make", "minimum-size keyboard recall preserves the exact query"
                assert book()["page"] == 3
                keyboard.key(0xFF0D)
                expect(2, "recalled query searches the reopened document")
                history(True)
                choose(3)  # Three queries, then Clear history.
                assert copied_query(True) == "Make", "clearing history retains the current query and focus"
                assert book()["page"] == 2
                # Empty History is disabled, so the seventh Tab reaches Close.
                for _ in range(7):
                    keyboard.key(0xFF09)
                keyboard.key(0xFF0D)
                keyboard.key("j", shift=True)
                expect(3, "clear disables History and keeps Close keyboard-accessible")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert document.read_bytes() == original
                assert "query_history" not in json.loads(config_path.read_text())
                print("PASS: recent explicit searches, keyboard menu recall, whole-query selection, fresh result navigation, no recall jump, same-window document reopen, 240x240 menu, clear retains query, disabled empty history, unchanged PDF and no persisted search history")
            except Exception:
                print(f"Process status: {application.poll()!r}", file=sys.stderr)
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
