#!/usr/bin/env python3
"""Exercise virtualized PDF previews in a real desktop. Run with xvfb-run -a."""
import ctypes
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
    with tempfile.TemporaryDirectory(prefix="pagewing-thumbnails-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = (fixture["PAGES"] * 14)[:80]
        # Preserve xref offsets while removing chapters, keeping controls near the top.
        data = re.sub(rb"/Outlines \d+ 0 R", lambda match: b" " * len(match.group()), bytes(fixture["make_pdf"]()), count=1)
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(data)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        keyboard = Keyboard()
        application = None
        with open(root / "application.log", "w+") as log:
            def launch():
                process = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
                wait_for(keyboard.focus, "thumbnail reader window")
                return process

            def saved(predicate):
                try:
                    state = json.loads(config_path.read_text())
                    return predicate(state, state["recent"][0])
                except (OSError, ValueError, KeyError, IndexError):
                    return False

            def expect(page, description):
                wait_for(lambda: saved(lambda _, book: book["page"] == page), description)

            def preview_ready():
                # The vector accent is thinner than two pixels at preview scale.
                for x in range(25, 85, 3):
                    for y in range(293, 310):
                        red, green, blue = keyboard.pixel((x, y))
                        if green > red * 1.4 and green > blue * 1.1:
                            return True
                return False

            def selected_bottom(height):
                # The outer sidebar may scroll when a clicked card gains focus.
                # Locate its actual accent fill instead of assuming a row y.
                rows = []
                for y in range(64, height - 36, 2):
                    red, green, blue = keyboard.pixel((16, y))
                    if green > red + 8 and green > blue + 6:
                        rows.append(y)
                return rows[-1] if rows else None

            def following_preview():
                bottom = selected_bottom(540)
                if bottom is not None:
                    for y in range(bottom + 4, 504, 2):
                        if keyboard.pixel((90, y)) == (251, 250, 246):
                            return 90, y
                return None

            try:
                application = launch()
                expect(0, "initial reading state")
                preferences_path = config_path.with_name("config.toml")
                preferences = preferences_path.read_text()
                keyboard.key("t")
                wait_for(lambda: saved(lambda state, _: state["thumbnails"]), "preview preference")
                wait_for(preview_ready, "actual PDF pixels in first preview")
                keyboard.click((90, 540))
                expect(1, "click second preview")
                keyboard.key("g", shift=True)
                expect(79, "Vim G reaches final page")
                time.sleep(0.3)
                keyboard.click((90, 310))
                expect(78, "preview list follows G to the final two pages")
                keyboard.key("g"); keyboard.key("g")
                expect(0, "Vim gg returns to first page")
                wait_for(preview_ready, "preview list follows gg")
                keyboard.wheel((90, 390), down=True, steps=9)
                time.sleep(0.4)
                keyboard.click((90, 340))
                wait_for(lambda: saved(lambda _, book: 1 < book["page"] < 20), "scroll and click a later preview")
                keyboard.key("g"); keyboard.key("g")
                wait_for(preview_ready, "return to cached first preview")
                keyboard.key("i")
                wait_for(lambda: sum(keyboard.pixel((90, 370))) < 100, "inverted preview pixels")
                keyboard.key("r")
                wait_for(lambda: saved(lambda _, book: book["rotation"] == 1), "rotated reading view")
                # Rotated landscape preview leaves empty space above its image.
                wait_for(lambda: sum(keyboard.pixel((90, 285))) > 400, "rotated preview aspect ratio")
                keyboard.key("r", shift=True); keyboard.key("i")
                wait_for(preview_ready, "restore upright preview colors")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                window = keyboard.focus()
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.4)
                keyboard.key("g", shift=True)
                expect(79, "last page in minimum window")
                # The padded header leaves room for one full preview here.
                # Scroll slightly to expose the preceding page before clicking it.
                keyboard.wheel((90, 350), steps=2)
                time.sleep(0.4)
                keyboard.click((90, 300))
                expect(78, "preceding preview remains clickable in minimum window")
                # Expose the next card after focus has revealed the preceding
                # one, then click rendered paper below the selected card.
                keyboard.wheel((90, 350), down=True, steps=2)
                keyboard.click(wait_for(following_preview, "visible last preview below the selected card"))
                expect(79, "last preview remains clickable in minimum window")
                application.terminate(); application.wait(timeout=5)
                assert preferences_path.read_text() == preferences, "temporary appearance changes must not rewrite TOML"
                application = launch()
                expect(79, "restore reading page")
                wait_for(lambda: saved(lambda state, _: not state["thumbnails"] and not state["sidebar"]), "restart reapplies configured appearance defaults")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                preferences_path.write_text("[appearance]\nsidebar = true\nthumbnails = true\n")
                application = launch()
                expect(79, "configuration edits preserve the reading page")
                wait_for(lambda: saved(lambda state, _: state["thumbnails"] and state["sidebar"]), "restart applies configured preview mode and sidebar")
                keyboard.key("g"); keyboard.key("g")
                wait_for(preview_ready, "restored preview mode renders PDF images")
                keyboard.key("t")
                wait_for(lambda: saved(lambda state, _: not state["thumbnails"]), "switch back to compact page list")
                window = keyboard.focus()
                keyboard.x11.XResizeWindow(keyboard.display, window, 1180, 820)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                keyboard.wheel((90, 390), down=True, steps=9)
                time.sleep(0.4)
                keyboard.key("g"); keyboard.key("g")
                bottom = wait_for(lambda: selected_bottom(820), "gg reveals the selected first compact row")
                keyboard.click((90, bottom + 10))
                expect(1, "gg reveals current page after manually scrolling the compact list")
                keyboard.key("g", shift=True)
                expect(79, "last page in compact list mode")
                keyboard.click((90, 290))
                expect(71, "compact list follows keyboard navigation to final pages")
                keyboard.click((90, 320))
                expect(72, "clicking a list row preserves its neighboring rows")
                keyboard.key(0xFF09)
                wait_for(lambda: saved(lambda state, _: not state["sidebar"]), "close sidebar")
                keyboard.key("t")
                wait_for(lambda: saved(lambda state, _: state["thumbnails"] and state["sidebar"]), "preview key reopens hidden sidebar")
                print("PASS: 80-page previews and compact list, rendered pixels, stable clicks, Vim tracking, same-page reveal, scrolling, inversion, rotation, compact window, reading progress and TOML appearance defaults")
            except Exception:
                log.flush(); log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application and application.poll() is None:
                    application.terminate(); application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
