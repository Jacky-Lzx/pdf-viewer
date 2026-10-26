#!/usr/bin/env python3
"""Exercise a thousand keyboard link hints in a native window. Use xvfb-run -a."""
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
    with tempfile.TemporaryDirectory(prefix="folio-link-hints-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        document.write_bytes(make_pdf(hint_links=1000))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
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

            def home():
                keyboard.key("g"); keyboard.key("g")
                expect(0, "return to dense link page")
                time.sleep(0.2)

            try:
                wait_for(keyboard.focus, "dense link reader window")
                expect(0, "initial reading state")
                time.sleep(0.3)
                keyboard.key("f")
                keyboard.key("b"); keyboard.key("m")
                keyboard.key(0xFF08)
                keyboard.key("m"); keyboard.key("l")
                expect(5, "final label bml selects link 1000 after Backspace")
                home()
                keyboard.key("f"); keyboard.key("a"); keyboard.key("a"); keyboard.key("b")
                expect(2, "label aab selects the second link")
                home()
                keyboard.key("f"); keyboard.key("b"); keyboard.key("m"); keyboard.key("z")
                keyboard.key("j", shift=True)
                expect(1, "invalid suffix cancels hints and normal Vim navigation resumes")
                home()
                keyboard.key("f"); keyboard.key("z")
                keyboard.key("j", shift=True)
                expect(1, "invalid initial letter cancels hints")
                home()
                keyboard.key("f"); keyboard.key("b"); keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(1, "Escape cancels a partial hint")
                home()
                keyboard.key("r")
                time.sleep(0.3)
                keyboard.key("f"); keyboard.key("b"); keyboard.key("m"); keyboard.key("l")
                expect(5, "all thousand link targets survive reading rotation")
                print("PASS: 1000 native link hints, partial groups, Backspace, invalid prefixes, Escape and rotated destinations")
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
