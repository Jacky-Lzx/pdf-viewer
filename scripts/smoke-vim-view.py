#!/usr/bin/env python3
"""Check Vim page alignment, preserved horizontal pan, rotation and saved view."""
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


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-vim-view-") as directory:
        root = Path(directory)
        document = root / "folio-smoke.pdf"
        shutil.copyfile(SCRIPTS.parent / "assets/folio-guide.pdf", document)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)

            def saved(predicate):
                try:
                    config = json.loads(config_path.read_text())
                    book = config["recent"][0]
                    return book if predicate(config, book) else None
                except (OSError, ValueError, KeyError, IndexError):
                    return None

            def position(page, vertical, description, fit=None, horizontal=None, rotation=0):
                return wait_for(lambda: saved(lambda config, book:
                    book["page"] == page and book["rotation"] == rotation
                    and abs(book["offset"][1] - vertical) < 0.001
                    and (fit is None or book["fit"] == fit)
                    and (horizontal is None or abs(book["offset"][0] - horizontal) < 0.001)
                    and not book["bookmarks"] and not config["thumbnails"]), description)

            def sequence(value, burst=False):
                if burst:
                    application.send_signal(signal.SIGSTOP)
                try:
                    for character in value:
                        keyboard.key(character.lower(), shift=character.isupper() or character == "+", delay=0 if burst else 0.025)
                finally:
                    if burst:
                        application.send_signal(signal.SIGCONT)

            try:
                wait_for(keyboard.focus, "Vim alignment reader window")
                position(0, 0, "initial fit page", fit="Page")
                sequence("a")
                position(0, 0.5, "100% zoom preserves the fitted page center", fit={"Custom": 1.0}, horizontal=0)
                sequence("+a", burst=True)
                position(0, 0.5, "same-frame relative and 100% zoom preserve the center", fit={"Custom": 1.0}, horizontal=0)
                sequence("p")
                position(0, 0, "return to fit page after 100% zoom", fit="Page")
                sequence("wj", burst=True)
                wait_for(lambda: saved(lambda _, book:
                    book["page"] == 0 and book["fit"] == "Width" and 0 < book["offset"][1] < 1),
                    "same-frame fit-width then scroll uses the new overflow without turning pages")
                sequence("zb")
                position(0, 1, "bottom before sequential zoom", fit="Width")
                sequence("+")
                reference = wait_for(lambda: saved(lambda _, book:
                    book["page"] == 0 and isinstance(book["fit"], dict) and 0 < book["offset"][1] < 1),
                    "sequential bottom then zoom preserves the viewport center")
                sequence("wzt")
                position(0, 0, "reset before batched bottom and zoom", fit="Width")
                sequence("zb+", burst=True)
                position(0, reference["offset"][1], "same-frame bottom then zoom matches sequential navigation",
                         reference["fit"], reference["offset"][0])
                sequence("wzt")
                position(0, 0, "reset before batched bottom and upward scroll", fit="Width")
                sequence("zbk", burst=True)
                wait_for(lambda: saved(lambda _, book:
                    book["page"] == 0 and book["fit"] == "Width" and 0 < book["offset"][1] < 0.999),
                    "same-frame alignment then scroll moves from the pending bottom position")
                sequence("wzbma")
                wait_for(lambda: saved(lambda _, book:
                    book["marks"].get("a", {}).get("offset", [0, 0])[1] == 1), "save the bottom position")
                sequence("J")
                position(1, 0, "leave the saved page", fit="Width")
                sequence("'a+", burst=True)
                position(0, reference["offset"][1], "same-frame restore and zoom use the saved position",
                         reference["fit"], reference["offset"][0])
                sequence("p")
                position(0, 0, "reset before alignment regression", fit="Page")
                sequence("wzb", burst=True)
                position(0, 1, "same-frame fit-width and zb reach the bottom", fit="Width")
                sequence("zz")
                position(0, 0.5, "zz reaches the vertical midpoint", fit="Width")
                sequence("zt")
                position(0, 0, "zt returns to top", fit="Width")
                sequence("zbzjJ")
                position(1, 0, "invalid z command is consumed before the following page command")
                sequence("z")
                keyboard.key(0xFF1B)
                sequence("J")
                position(2, 0, "Escape cancels z prefix without swallowing following navigation")
                sequence("9999999zbJ")
                position(3, 0, "alignment consumes numeric prefix before the next page command")
                sequence("+zb", burst=True)
                position(3, 1, "same-frame zoom and alignment preserve the centered horizontal anchor", horizontal=0.5)
                sequence("l")
                before = wait_for(lambda: saved(lambda _, book:
                    book["page"] == 3 and isinstance(book["fit"], dict)
                    and 0.5 < book["offset"][0] < 1), "zoomed horizontal pan after moving right")
                fit, horizontal = before["fit"], before["offset"][0]
                for command, target in [("zb", 1), ("zt", 0), ("zz", 0.5)]:
                    sequence(command)
                    position(3, target, f"{command} preserves zoom and horizontal position", fit, horizontal)
                sequence("rl")
                before = wait_for(lambda: saved(lambda _, book:
                    book["rotation"] == 1 and 0 < book["offset"][0] < 1), "rotated horizontal pan")
                horizontal = before["offset"][0]
                for command, target in [("zb", 1), ("zz", 0.5), ("zt", 0)]:
                    sequence(command)
                    position(3, target, f"{command} follows the rotated page axes", fit, horizontal, rotation=1)
                sequence("zb")
                position(3, 1, "save bottom of rotated page", fit, horizontal, rotation=1)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                application = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "restored rotated reading window")
                time.sleep(0.5)
                sequence("zt")
                position(3, 0, "restored zoom and horizontal position remain navigable", fit, horizontal, rotation=1)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: same-frame fit/scroll/alignment/zoom/restore, zt/zz/zb, invalid/cancelled prefixes, counts, preserved zoom/pan, rotated axes and restart")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.send_signal(signal.SIGCONT)
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
