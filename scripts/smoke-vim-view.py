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
                        keyboard.key(character.lower(), shift=character.isupper() or character in "+$", delay=0 if burst else 0.025)
                finally:
                    if burst:
                        application.send_signal(signal.SIGCONT)

            try:
                wait_for(keyboard.focus, "Vim alignment reader window")
                position(0, 0, "initial fit page", fit="Page")
                sequence("999zLJ")
                position(1, 0, "horizontal half-screen stops on a fitted page and consumes its count", fit="Page", horizontal=0)
                sequence("K999z$J", burst=True)
                position(1, 0, "horizontal edge alignment stops on a fitted page and consumes its count", fit="Page", horizontal=0)
                sequence("$J", burst=True)
                position(2, 0, "a bare dollar cannot become a hidden numeric prefix", fit="Page", horizontal=0)
                sequence("2K")
                sequence("w++++++999hzz", burst=True)
                zoomed = position(0, 0.5, "prepare zoomed horizontal scrolling", horizontal=0)
                horizontal_step = 0.5 / (1.2 ** 6 - 1)
                sequence("zL")
                position(0, 0.5, "zL moves exactly half the viewport and preserves vertical position", zoomed["fit"], horizontal_step)
                sequence("2zH")
                position(0, 0.5, "counted zH clamps at the left edge", zoomed["fit"], 0)
                sequence("2zL")
                position(0, 0.5, "numeric prefix multiplies horizontal half-screen distance", zoomed["fit"], 2 * horizontal_step)
                sequence("ma999zLJ")
                position(1, 0, "horizontal edge never turns pages or leaks the numeric prefix", zoomed["fit"], 0)
                sequence("'azH", burst=True)
                position(0, 0.5, "same-frame mark restore and zH scroll from the saved position", zoomed["fit"], horizontal_step)
                sequence("999zL")
                position(0, 0.5, "large horizontal count clamps at the right edge", zoomed["fit"], 1)
                sequence("zH")
                position(0, 0.5, "zH reverses half a screen from the right edge", zoomed["fit"], 1 - horizontal_step)
                sequence("z$")
                position(0, 0.5, "z$ aligns to the right edge without changing vertical position", zoomed["fit"], 1)
                sequence("z0")
                position(0, 0.5, "z0 aligns to the left edge without changing vertical position", zoomed["fit"], 0)
                sequence("4z$J", burst=True)
                position(1, 0, "horizontal alignment consumes counts before subsequent page navigation", zoomed["fit"], 0)
                sequence("'az$", burst=True)
                position(0, 0.5, "same-frame saved view and right alignment preserve the saved vertical position", zoomed["fit"], 1)
                sequence("z$ztz0", burst=True)
                position(0, 0, "same-frame horizontal and vertical alignment compose", zoomed["fit"], 0)
                sequence("zz")
                sequence("rzz999h")
                position(0, 0.5, "prepare rotated horizontal scrolling", zoomed["fit"], 0, rotation=1)
                sequence("zL")
                position(0, 0.5, "zL follows the rotated horizontal axis", zoomed["fit"],
                         0.5 / (800 / 600 * 1.2 ** 6 - 1), rotation=1)
                sequence("z$")
                position(0, 0.5, "right alignment follows the rotated page axis", zoomed["fit"], 1, rotation=1)
                sequence("z0")
                position(0, 0.5, "left alignment follows the rotated page axis", zoomed["fit"], 0, rotation=1)
                keyboard.key("r", shift=True)
                sequence("p")
                position(0, 0, "reset after horizontal half-screen navigation", fit="Page", horizontal=0)
                sequence("3r")
                position(0, 0, "three clockwise quarter turns", fit="Page", rotation=3)
                sequence("3R")
                position(0, 0, "three counterclockwise quarter turns", fit="Page", rotation=0)
                sequence("wzb4r", burst=True)
                position(0, 0, "a complete turn consumes the pending bottom position and resets the origin", fit="Width", rotation=0)
                sequence("J")
                position(1, 0, "a rotation count cannot leak into the next page command", fit="Width")
                sequence("K9999999r")
                position(0, 0, "large rotation count directly resolves its final orientation", fit="Width", rotation=3)
                sequence("9999999R")
                position(0, 0, "large inverse rotation restores the orientation", fit="Width", rotation=0)
                sequence("9")
                keyboard.click((700, 28))
                position(0, 0, "toolbar rotation clears a pending keyboard count", fit="Width", rotation=1)
                sequence("J")
                position(1, 0, "toolbar count cleanup preserves single-page navigation", fit="Width", rotation=1)
                keyboard.key("r", shift=True)
                sequence("K")
                position(0, 0, "reset after counted rotations", fit="Width", rotation=0)
                sequence("wzb")
                position(0, 1, "bottom before keyboard rotation", fit="Width")
                keyboard.key("r")
                position(0, 0, "keyboard rotation starts the rotated page at its origin", fit="Width", rotation=1)
                keyboard.key("r", shift=True)
                sequence("zb")
                position(0, 1, "bottom before toolbar rotation", fit="Width")
                keyboard.click((700, 28))
                position(0, 0, "toolbar rotation matches keyboard origin", fit="Width", rotation=1)
                keyboard.key("r", shift=True)
                sequence("zbmaJ")
                position(1, 0, "leave a saved bottom mark before rotation", fit="Width")
                sequence("'ar", burst=True)
                position(0, 0, "same-frame mark restore and rotation consume the saved offset before resetting pan", fit="Width", rotation=1)
                keyboard.key("r", shift=True)
                sequence("p")
                position(0, 0, "reset after rotation consistency checks", fit="Page")
                sequence("999")
                keyboard.key("e", ctrl=True)
                sequence("J")
                position(1, 0, "Ctrl+e stops on a fitted page and consumes its count", fit="Page")
                keyboard.key("y", ctrl=True)
                sequence("K")
                position(0, 0, "Ctrl+y stops on a fitted page without turning backwards", fit="Page")
                sequence("wzt")
                position(0, 0, "start counted page-only scrolling", fit="Width")
                keyboard.key("e", ctrl=True)
                line = wait_for(lambda: saved(lambda _, book:
                    book["page"] == 0 and book["fit"] == "Width" and 0 < book["offset"][1] < 0.25),
                    "Ctrl+e scrolls within the current page")["offset"][1]
                keyboard.key("y", ctrl=True)
                position(0, 0, "Ctrl+y reverses one scroll step", fit="Width")
                sequence("3")
                keyboard.key("e", ctrl=True)
                position(0, 3 * line, "numeric prefix multiplies Ctrl+e scrolling", fit="Width")
                sequence("2")
                keyboard.key("y", ctrl=True)
                position(0, line, "numeric prefix multiplies Ctrl+y scrolling", fit="Width")
                application.send_signal(signal.SIGSTOP)
                try:
                    sequence("zb", burst=False)
                    keyboard.key("e", ctrl=True, delay=0)
                    keyboard.key("y", ctrl=True, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                position(0, 1 - line, "same-frame bottom and Ctrl+e/y use the resolved edge without turning pages", fit="Width")
                sequence("9999999")
                keyboard.key("e", ctrl=True)
                position(0, 1, "large Ctrl+e count clamps at the bottom", fit="Width")
                sequence("9999999")
                keyboard.key("y", ctrl=True)
                position(0, 0, "large Ctrl+y count clamps at the top", fit="Width")
                sequence("J")
                position(1, 0, "page-only scrolling consumes its count before the next page command", fit="Width")
                sequence("ggp")
                position(0, 0, "reset after page-only scrolling", fit="Page")
                sequence("a")
                position(0, 0.5, "100% zoom preserves the fitted page center", fit={"Custom": 1.0}, horizontal=0)
                sequence("+a", burst=True)
                position(0, 0.5, "same-frame relative and 100% zoom preserve the center", fit={"Custom": 1.0}, horizontal=0)
                keyboard.key("+", ctrl=True, shift=True)
                position(0, 0.5, "Ctrl+Plus zooms in at the viewport center", fit={"Custom": 1.2}, horizontal=0)
                keyboard.key("-", ctrl=True)
                position(0, 0.5, "Ctrl+Minus reverses the zoom step", fit={"Custom": 1.0}, horizontal=0)
                keyboard.key("=", ctrl=True)
                position(0, 0.5, "Ctrl+Equals also zooms in without requiring Shift", fit={"Custom": 1.2}, horizontal=0)
                sequence("9")
                keyboard.key("0", ctrl=True)
                sequence("J")
                position(1, 0, "Ctrl+0 resets 100% zoom and clears a pending count", fit={"Custom": 1.0})
                sequence("ggm")
                keyboard.key("0", ctrl=True)
                sequence("J")
                position(1, 0, "Ctrl+0 cancels a pending mark prompt before reading navigation", fit={"Custom": 1.0})
                sequence("ggp")
                position(0, 0, "prepare resetting fitted zoom with Ctrl+0", fit="Page")
                keyboard.key("0", ctrl=True)
                position(0, 0.5, "Ctrl+0 preserves the fitted page center", fit={"Custom": 1.0}, horizontal=0)
                input_background = keyboard.pixel((275, 450))
                sequence("/jkg0-=")
                keyboard.key("0", ctrl=True)
                keyboard.key("-", ctrl=True)
                keyboard.key("=", ctrl=True)
                keyboard.key("+", ctrl=True, shift=True)
                keyboard.key("a", ctrl=True); keyboard.key("c", ctrl=True)
                assert keyboard.clipboard() == "jkg0-=", "zoom shortcuts must preserve search input focus and contents"
                time.sleep(0.3)
                assert keyboard.pixel((275, 450)) == input_background, "zoom shortcuts in text inputs must not resize the whole interface"
                keyboard.key(0xFF1B)
                sequence("J")
                position(1, 0, "zoom shortcuts in text inputs do not change the document scale", fit={"Custom": 1.0})
                sequence("gg")
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
                print("PASS: counted clockwise/counterclockwise and full-turn rotations, large counts and toolbar cleanup, counted zH/zL half-screen distance, z0/z$ edges and same-frame restored alignment, standard zoom shortcuts and input/UI-scale guards, edge clamping, restored/rotated horizontal scrolling, toolbar/keyboard and same-frame restored rotation, counted Ctrl+e/y with no edge page turns, same-frame fit/scroll/alignment/zoom/restore, zt/zz/zb, invalid/cancelled prefixes, preserved zoom/pan and restart")
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
