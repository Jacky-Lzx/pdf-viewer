#!/usr/bin/env python3
"""Check dense substring results and the whole-word global cap after reload."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-search-dense-") as directory:
        root = Path(directory)
        document = root / "pagewing-smoke.pdf"
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))

        def write_document(count):
            fixture["PAGES"][:] = [
                ("First page", "SEARCH BATCHES", [("dense " * 30).strip()] * (count // 30)),
                ("Last page", "SEARCH BATCHES", ["dense"]),
            ]
            data = re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()), bytes(fixture["make_pdf"]()), count=1)
            document.write_bytes(data)
            # Verify actual extracted text, including content outside the crop.
            result = subprocess.run([str(binary), "--text", str(document), "1"], capture_output=True, text=True, check=True)
            assert len(re.findall(r"\bdense\b", result.stdout)) == count

        write_document(1500)
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

            def counted(count, reverse=False):
                for digit in str(count):
                    keyboard.key(digit, delay=0.025)
                keyboard.key("n", shift=reverse)

            def find():
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                for letter in "dense":
                    keyboard.key(letter, delay=0.025)
                keyboard.key(0xFF0D)
                time.sleep(0.7)
                keyboard.key(0xFF1B)

            try:
                wait_for(keyboard.focus, "dense search reader window")
                expect(0, "initial first page")
                find()
                counted(1000)
                counted(500)
                expect(1, "1500 counted jumps reach the next page only after all 1500 first-page hits")
                counted(500, reverse=True)
                expect(0, "reverse to first-page match number 1001")
                counted(1001, reverse=True)
                expect(1, "reverse counted wrap includes every first-page match")
                keyboard.key("f", ctrl=True)
                keyboard.key("w", alt=True)
                wait_for(lambda: json.loads(config_path.read_text())["search_options"]["whole_word"], "whole-word mode saved before dense reload")
                keyboard.key(0xFF1B)
                write_document(12000)
                keyboard.key("r", ctrl=True)
                time.sleep(0.7)
                keyboard.key("g"); keyboard.key("g")
                expect(0, "first page after expanding the PDF")
                find()
                keyboard.key("n", shift=True)
                # A false wrap to the last page must have time to reach autosave.
                time.sleep(2.5)
                expect(0, "global 10000-hit cap ends within first page, never at the later-page match")
                keyboard.key("n")
                keyboard.key("j", shift=True)
                expect(1, "reading navigation remains usable after reaching the cap")
                keyboard.key("n")
                expect(0, "next result returns to first-page matches after manual navigation")
                keyboard.key("f", ctrl=True)
                keyboard.key("a", ctrl=True)
                for letter in "absent":
                    keyboard.key(letter, delay=0.025)
                keyboard.key(0xFF0D)
                time.sleep(0.5)
                keyboard.key(0xFF1B)
                keyboard.key("j", shift=True)
                expect(1, "new no-match query cancels dense results and pending jumps")
                print("PASS: 1501 substring matches across two pages, first-page match 1001, counted wrap, whole-word reload to 12000 first-page matches, global cap and replacement search")
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
