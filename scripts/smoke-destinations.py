#!/usr/bin/env python3
"""Exercise PDF target positions, view modes and same-page jumps under Xvfb."""
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
make_pdf = runpy.run_path(str(SCRIPTS / "make-sample.py"))["make_pdf"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-destinations-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        document.write_bytes(make_pdf(destination_views={1: "/XYZ 48 500 2", 2: "/FitR 100 100 300 300", 3: "/FitH 600", 4: "/FitV 250", 5: "/XYZ null 650 null"}))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def expect(predicate, description):
                def saved():
                    try:
                        return predicate(json.loads(config_path.read_text())["recent"][0])
                    except (OSError, ValueError, KeyError, IndexError, TypeError):
                        return False
                wait_for(saved, description)

            def custom(book):
                return book["fit"].get("Custom", 0) if isinstance(book["fit"], dict) else 0

            try:
                wait_for(keyboard.focus, "destination reader window")
                expect(lambda book: book["page"] == 0, "initial page")
                keyboard.key("f"); keyboard.key("a")
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7, "Vim link hints apply XYZ target zoom and location")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                keyboard.click((460, 626))
                expect(lambda book: book["page"] == 1 and custom(book) == 2 and 0.6 < book["offset"][1] < 0.7, "mouse link uses the same target location")
                keyboard.key(0xFF09); time.sleep(0.3)
                keyboard.key("1"); keyboard.key("0"); keyboard.key("0"); keyboard.key("k")
                expect(lambda book: book["page"] == 1 and book["offset"][1] == 0, "move away from the target on its own page")
                keyboard.click((100, 237))
                expect(lambda book: book["page"] == 1 and 0.6 < book["offset"][1] < 0.7, "same-page outline action repositions the viewport")
                keyboard.key("r"); keyboard.click((100, 237))
                expect(lambda book: book["page"] == 1 and book["rotation"] == 1 and 0.15 < book["offset"][1] < 0.25, "XYZ coordinates follow reading rotation")
                keyboard.key("r", shift=True)
                keyboard.click((100, 272))
                expect(lambda book: book["page"] == 2 and 3 < custom(book) < 4 and 0.80 < book["offset"][1] < 0.87, "named rectangular destination fits and centers its region")
                keyboard.click((100, 377))
                expect(lambda book: book["page"] == 5 and 3 < custom(book) < 4 and 0.23 < book["offset"][1] < 0.27, "nullable XYZ destination preserves zoom and sets the requested top")
                keyboard.click((100, 307))
                expect(lambda book: book["page"] == 3 and book["fit"] == "Width" and 0.5 < book["offset"][1] < 0.65, "named FitH destination applies width fit and top coordinate")
                keyboard.click((100, 342))
                expect(lambda book: book["page"] == 4 and 0.8 < custom(book) < 0.9 and book["offset"][1] == 0, "FitV destination fits page height")
                keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
                time.sleep(0.2)
                keyboard.click((100, 377))
                expect(lambda book: book["page"] == 5 and 0.8 < custom(book) < 0.9, "nullable target zoom retains the actual scale from an automatic fit mode")
                print("PASS: mouse/Vim/outline destinations, XYZ zoom, same-page jumps, rotation, named FitR/FitH targets and nullable coordinates")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                application.terminate()
                application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
