#!/usr/bin/env python3
"""Check word/line selection and real clipboard content. Run with xvfb-run -a."""
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]
SENTENCE = "A native PDF reader, written in Rust."


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-selection-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        shutil.copyfile(SCRIPTS.parent / "assets/folio-guide.pdf", document)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
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
                keyboard.key(0xFF1B)
                keyboard.key("r", shift=True); keyboard.key("w")
                wait_for(lambda: saved(lambda book: book["fit"] == "Width"), "normal mode fit width")
                clicks((170, 540), 2)
                wait_for(lambda: saved(lambda book: book["fit"] == "Page"), "normal double-click still fits the page")
                print("PASS: word/line selection, forward/reverse unit drags, real clipboard, single-click reset, rotated text and preserved selection zoom")
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
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
