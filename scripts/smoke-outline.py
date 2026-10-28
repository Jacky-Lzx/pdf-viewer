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
    with tempfile.TemporaryDirectory(prefix="pagewing-outline-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        source = make_pdf(nested_outline=True)
        original = b"/Names [(search) [11 0 R /Fit]]"
        malformed = b"/Names[0 0(search)[11 0 R/Fit]]"
        assert original in source and len(original) == len(malformed)
        # Keep every xref offset intact while inserting an invalid key/value
        # before the valid named destination used by a later chapter.
        source = source.replace(original, malformed, 1)
        original = b"/Kids [25 0 R 26 0 R]"
        malformed = b"/Kids [null   26 0 R]"
        assert original in source and len(original) == len(malformed)
        # A bad first number-tree child must not hide the later A- label range.
        source = source.replace(original, malformed, 1)
        original = b"/Nums [2 << /S /D >> 4 "
        malformed = b"/Nums [2 999 0 R     4 "
        assert original in source and len(original) == len(malformed)
        # A dangling value must not hide the valid label pair that follows it.
        document.write_bytes(source.replace(original, malformed, 1))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
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
                keyboard.click((90, 211))
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
                keyboard.click((100, 312))
                expect(2, "third-level named destination")
                keyboard.click((36, 275))
                keyboard.click((100, 312))
                expect(3, "folded chapter hides its child and keeps its next sibling")
                keyboard.click((24, 240))
                keyboard.click((90, 277))
                expect(4, "folding a root preserves other root chapters")
                filter_chapters("yourself")
                expect(4, "chapter typing does not trigger navigation or rotation")
                keyboard.key(0xFF1B)
                keyboard.click((100, 312))
                expect(2, "filter reveals matching title and both folded ancestors")
                keyboard.click((228, 211))
                keyboard.click((90, 277))
                expect(4, "clearing a filter restores the previous root fold")
                keyboard.click((190, 185))
                keyboard.click((100, 312))
                expect(2, "expand all restores nested chapters")
                keyboard.click((228, 185))
                keyboard.click((90, 277))
                expect(4, "collapse all keeps both root chapters")
                filter_chapters("no-such-chapter")
                keyboard.key(0xFF1B)
                keyboard.click((228, 211))
                filter_chapters("yourself")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.key(0xFF1B)
                keyboard.click((100, 312))
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
                # Real UTF-16 PDF metadata, preserving every xref byte offset.
                source = document.read_bytes()
                for original, replacement in [
                    (b"/Title (A little room to read.)", b"/Title (" + b"\xfe\xff" + "Straße".encode("utf-16-be") + b")"),
                    (b"/S /D /P (A-)", b"/S/D/P(" + b"\xfe\xff" + "ß".encode("utf-16-be") + b")"),
                ]:
                    assert original in source and len(replacement) <= len(original)
                    source = source.replace(original, replacement.ljust(len(original)), 1)
                document.write_bytes(source)
                keyboard.key("r", ctrl=True)
                time.sleep(0.5)
                burst(chapter_keys("strasse") + [(0xFF0D, {})])
                expect(0, "UTF-16 title decodes before Unicode case-folded filtering")
                keyboard.key(":", shift=True)
                for letter in "SS2":
                    keyboard.key(letter.lower(), shift=letter.isupper())
                keyboard.key(0xFF0D)
                expect(5, "UTF-16 page-label prefix accepts Unicode case expansion")
                # A later exact label must win over an earlier Unicode-folded
                # candidate. Rebuild from the original fixture, retaining xrefs.
                source = make_pdf(nested_outline=True)
                original = b"/Nums [2 << /S /D >> 4 << /S /D /P (A-) >>]"
                replacement = b"/Nums[2<</S/D/P(" + b"\xfe\xff" + "ß".encode("utf-16-be") + b")>>4<</S/D/P(SS)>>]"
                assert original in source and len(replacement) <= len(original)
                document.write_bytes(source.replace(original, replacement.ljust(len(original)), 1))
                keyboard.key("r", ctrl=True)
                time.sleep(0.5)

                def label_input(value):
                    keyboard.key(":", shift=True)
                    for letter in value:
                        keyboard.key(letter.lower(), shift=letter.isupper())
                    keyboard.key(0xFF0D)

                label_input("ss2")
                expect(3, "first Unicode-folded duplicate wins without an exact label")
                label_input("SS2")
                expect(5, "later exact label takes priority over earlier folded duplicate")
                label_input("SS02")
                time.sleep(0.3)
                expect(5, "noncanonical numeric suffix does not navigate")
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "SS02", "invalid label remains editable"
                for letter in "SS1":
                    keyboard.key(letter.lower(), shift=letter.isupper())
                keyboard.key(0xFF0D)
                expect(4, "correcting an invalid label preserves exact-match priority")
                print("PASS: malformed name-pair/number-tree child recovery, dangling label-pair recovery, nested folds, real UTF-16 titles/page labels, Unicode filtering, exact/folded duplicate label priority, invalid label correction, ancestor context, compact layout, same-frame chapter input/Enter, keyboard selection, hidden sidebar, no-match handling and focus switching")
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
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
