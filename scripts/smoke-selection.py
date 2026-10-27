#!/usr/bin/env python3
"""Check word/line selection and real clipboard content. Run with xvfb-run -a."""
import json
import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
SENTENCE = "A native PDF reader, written in Rust."


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-selection-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        shutil.copyfile(SCRIPTS.parent / "assets/pagewing-guide.pdf", document)
        page_text = [subprocess.run([str(binary), "--text", str(document), str(page)], check=True, capture_output=True).stdout.decode("utf-8").removesuffix("\n") for page in (1, 2)]
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def saved(predicate):
                try:
                    return predicate(json.loads(config_path.read_text())["recent"][0])
                except (OSError, ValueError, KeyError, IndexError):
                    return False

            def clicks(position, count):
                time.sleep(0.4)
                for _ in range(count):
                    keyboard.click(position)

            def copy(expected):
                keyboard.key("c", ctrl=True)
                actual = keyboard.clipboard()
                assert actual == expected, (actual, expected)

            try:
                wait_for(keyboard.focus, "selection reader window")
                wait_for(lambda: saved(lambda book: book["page"] == 0), "initial reading state")
                application.send_signal(signal.SIGSTOP)
                try:
                    keyboard.key("a", ctrl=True, delay=0)
                    keyboard.key("c", ctrl=True, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)
                assert keyboard.clipboard() == page_text[0], "same-frame Select All and Copy works before text arrives"
                clicks((400, 290), 1)
                keyboard.key("y")
                assert len(keyboard.clipboard()) == 1, "clicking starts a fresh glyph selection after Select All"
                keyboard.key("a", ctrl=True)
                keyboard.key("a", ctrl=True)
                keyboard.key("y")
                assert keyboard.clipboard() == page_text[0], "Select All is repeatable and y copies the complete page"
                keyboard.key("v")
                keyboard.key("v")
                time.sleep(0.3)
                clicks((400, 290), 2)
                copy("native")
                keyboard.drag((400, 290), (450, 290))
                copy("native PDF reader")
                clicks((450, 290), 2)
                keyboard.drag((450, 290), (400, 290))
                copy("native PDF reader")
                clicks((400, 290), 3)
                copy(SENTENCE)
                keyboard.drag((400, 290), (430, 315))
                copy(SENTENCE + "\nDesigned for Linux and macOS. Made for your keyboard.")
                clicks((400, 290), 1)
                selected = keyboard.clipboard()
                keyboard.key("c", ctrl=True)
                wait_for(lambda: keyboard.clipboard() != selected, "single click resets line selection")
                assert len(keyboard.clipboard()) == 1
                keyboard.key("w")
                wait_for(lambda: saved(lambda book: book["fit"] == "Width"), "fit width")
                clicks((170, 540), 2)
                copy("native")
                wait_for(lambda: saved(lambda book: book["fit"] == "Width"), "double-click in selection mode preserves zoom")
                keyboard.key("p"); keyboard.key("r")
                wait_for(lambda: saved(lambda book: book["rotation"] == 1), "reading rotation")
                clicks((768, 175), 2)
                copy("native")
                clicks((768, 175), 3)
                copy(SENTENCE)
                keyboard.key("a", ctrl=True)
                copy(page_text[0])
                keyboard.key(0xFF1B)
                keyboard.key("r", shift=True); keyboard.key("w")
                wait_for(lambda: saved(lambda book: book["fit"] == "Width"), "normal mode fit width")
                clicks((170, 540), 2)
                time.sleep(0.4)
                assert saved(lambda book: book["fit"] == "Width"), "normal double-click preserves zoom"
                keyboard.key("p")
                wait_for(lambda: saved(lambda book: book["fit"] == "Page"), "fit page shortcut")
                # Clipboard still holds the sentence, so an empty query cannot
                # accidentally pass the search-field copy check with stale text.
                keyboard.key("v")
                time.sleep(0.3)
                clicks((400, 290), 2)
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                copy("native")
                keyboard.key(0xFF0D); keyboard.key(0xFF1B)
                keyboard.key("v")
                time.sleep(0.3)
                clicks((400, 290), 3)
                keyboard.key("/")
                keyboard.key("a", ctrl=True)
                copy(SENTENCE)
                # A multi-line selection keeps the previous query.
                keyboard.key("k"); keyboard.key("e"); keyboard.key("p"); keyboard.key("t")
                keyboard.key(0xFF1B)
                keyboard.key("v")
                time.sleep(0.3)
                clicks((400, 290), 3)
                keyboard.drag((400, 290), (430, 315))
                copy(SENTENCE + "\nDesigned for Linux and macOS. Made for your keyboard.")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                copy("kept")
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                wait_for(lambda: saved(lambda book: book["page"] == 1), "second page for Select All")
                keyboard.key("a", ctrl=True)
                copy(page_text[1])
                keyboard.key("k", shift=True)
                wait_for(lambda: saved(lambda book: book["page"] == 0), "page navigation cancels Select All")
                keyboard.key("y")
                assert keyboard.clipboard() == page_text[0], "page change must not copy a stale full-page selection"
                # One drawn glyph can represent thousands of Unicode scalars.
                # Selection preparation must use text size as well as glyphs.
                fixture = runpy.run_path(str(SCRIPTS / "make-selection-fixture.py"))
                document.write_bytes(fixture["make_pdf"]())
                keyboard.key("r", ctrl=True)
                def reloaded_long_text():
                    keyboard.key("y")
                    return keyboard.clipboard() == fixture["TEXT"]
                wait_for(reloaded_long_text, "reload a sparse glyph page with a long Unicode text layer")
                keyboard.key("v")
                clicks((382, 138), 2)
                copy(fixture["TEXT"])
                clicks((382, 138), 3)
                copy(fixture["TEXT"])
                keyboard.key(0xFF1B)
                print("PASS: same-frame Select All/Copy before text arrives, repeat/glyph reset, rotated/current-page full text, page-change cancellation, word/line drags, long Unicode glyph mappings, clipboard, preserved zoom and search input/prefill guards")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
