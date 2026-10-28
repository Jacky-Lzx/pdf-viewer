#!/usr/bin/env python3
"""Check welcome-card keyboard focus, resizing and manual scrolling in X11."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-welcome-") as directory:
        root = Path(directory)
        paths = [root / f"Recent-{index}.pdf" for index in range(4)]
        for path in paths:
            path.write_bytes((SCRIPTS.parent / "assets/pagewing-guide.pdf").read_bytes())
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"recent": [
            {"path": str(path), "page": index} for index, path in enumerate(paths)
        ]}))
        # A disabled reader binding must still leave welcome focus usable.
        config_path.with_name("config.toml").write_text("[shortcuts]\nsidebar = []\n")
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        keyboard.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int,
                                                      ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        size = (240, 240)

        def park_pointer():
            keyboard.xtst.XTestFakeMotionEvent(keyboard.display, -1, 1270, 1000, 0)
            keyboard.x11.XFlush(keyboard.display)

        def resize(width, height):
            nonlocal size
            size = (width, height)
            keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
            park_pointer()
            time.sleep(0.3)

        def focused_card():
            width, height = size
            rows = []
            for y in range(56, height - 32):
                colors = [keyboard.pixel((width // 2 + offset, y)) for offset in (-60, 0, 60)]
                if all(green > red + 8 and green > blue + 6 for red, green, blue in colors):
                    rows.append(y)
            bands = []
            for y in rows:
                if bands and y == bands[-1][-1] + 1:
                    bands[-1].append(y)
                else:
                    bands.append([y])
            # A card has two thin borders. Ignore the solid green logo and
            # Open button, and require the whole card inside the reader area.
            lines = [sum(band) / len(band) for band in bands if len(band) <= 3]
            for top, bottom in zip(lines, lines[1:]):
                if 40 < bottom - top < 150:
                    return top, bottom
            return None

        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary)], env=environment, stdout=log, stderr=log)
            try:
                window = wait_for(lambda: keyboard.focus("Pagewing"), "welcome window")
                resize(240, 240)
                for _ in range(20):
                    keyboard.key(0xFF09)
                    time.sleep(0.1)
                    if focused_card():
                        break
                else:
                    raise AssertionError("Tab never revealed a complete recent-document card")
                for _ in range(3):
                    keyboard.key(0xFF09)
                    wait_for(focused_card, "the next focused recent card is fully visible")
                for dimensions in ((240, 400), (860, 240), (1180, 820), (240, 240)):
                    resize(*dimensions)
                    wait_for(focused_card, f"focused card survives resize to {dimensions}")
                top, bottom = wait_for(focused_card, "focused card before manual scrolling")
                keyboard.wheel((120, round((top + bottom) / 2)), steps=20)
                park_pointer()
                wait_for(lambda: focused_card() is None, "manual scrolling can hide the focused card")
                time.sleep(0.3)
                assert focused_card() is None, "ordinary frames must retain manual scrolling"
                resize(240, 400)
                wait_for(focused_card, "resizing reveals the retained keyboard focus")
                keyboard.key(0xFF0D)

                def opened():
                    try:
                        item = json.loads(config_path.read_text())["recent"][0]
                        return item["path"] == str(paths[3]) and item["page"] == 3
                    except (OSError, ValueError, KeyError, IndexError):
                        return False

                wait_for(opened, "Enter opens the focused fourth card at its saved page")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: welcome Tab focus, four visible recent cards, minimum/tall/wide resizing, manual scroll preservation, focus restored on resize and Enter opens the correct saved page")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
