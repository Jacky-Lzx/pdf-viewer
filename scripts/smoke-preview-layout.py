#!/usr/bin/env python3
"""Check responsive PDF previews, long sidebars and actual pixels in X11."""
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
    with tempfile.TemporaryDirectory(prefix="pagewing-preview-layout-") as directory:
        root = Path(directory)
        fixture = runpy.run_path(str(SCRIPTS / "make-sample.py"))
        fixture["PAGES"][:] = [
            (f"Chapter {page + 1:03}", "PREVIEW LAYOUT", ["Responsive previews."])
            for page in range(80)
        ]
        document = root / "pagewing-smoke.pdf"
        document.write_bytes(fixture["make_pdf"](
            page_label_prefix="Long appendix 中文😀 with a complete label suffix "))
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({
            "sidebar": True,
            "recent": [{"path": str(document), "bookmarks": list(range(60))}],
        }))
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        keyboard = Keyboard()
        current_width = 860
        keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                              ctypes.c_uint, ctypes.c_uint]
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(document)], env=environment,
                                           stdout=log, stderr=log)

            def saved():
                try:
                    return json.loads(config_path.read_text())["recent"][0]
                except (OSError, ValueError, KeyError, IndexError):
                    return {}

            def expect(page, description):
                wait_for(lambda: saved().get("page") == page, description)

            def resize(width, height):
                nonlocal current_width
                current_width = width
                keyboard.x11.XResizeWindow(keyboard.display, window, width, height)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)

            def image_point(height, selected=True, night=False):
                # The fixture's paper color differs from egui placeholders.
                # Scan across the card: short windows shrink the image around
                # its center even when the sidebar itself remains wide.
                paper = (4, 5, 9) if night else (251, 250, 246)
                right = min(242, max(100, current_width - 120) - 8)
                for y in range(112, height - 42, 2):
                    red, green, blue = keyboard.pixel((16, y))
                    if selected and not (green > red + 8 and green > blue + 6):
                        continue
                    for x in range(20, right, 3):
                        if keyboard.pixel((x, y)) == paper:
                            return x, y
                return None

            try:
                window = wait_for(keyboard.focus, "responsive preview window")
                expect(0, "initial page")
                resize(860, 540)
                keyboard.key("g", shift=True)
                expect(79, "last page before opening previews")
                keyboard.key("t")
                wait_for(lambda: image_point(540), "actual current preview below 80 chapters and 60 bookmarks")
                for width, height in ((240, 240), (860, 240), (1180, 820), (240, 400)):
                    resize(width, height)
                    wait_for(lambda: image_point(height), f"selected preview pixels after resizing to {width}x{height}")
                    expect(79, "resizing preserves reading page")
                keyboard.key("r")
                wait_for(lambda: saved().get("rotation") == 1, "rotation applied")
                wait_for(lambda: image_point(400), "landscape preview still renders at narrow width")
                keyboard.key("i")
                wait_for(lambda: image_point(400, night=True), "inverted responsive preview pixels")
                keyboard.key("i")
                keyboard.key("r", shift=True)
                position = wait_for(lambda: image_point(400), "upright preview restored")
                keyboard.wheel(position, steps=8)
                time.sleep(0.3)
                assert not image_point(400), "ordinary frames must retain manual preview scrolling"
                resize(240, 240)
                assert not image_point(240), "resizing must retain the manually chosen pages"
                position = wait_for(lambda: image_point(240, selected=False), "a scrolled preview image is visible")
                keyboard.click(position)
                wait_for(lambda: 60 < saved().get("page", 0) < 79,
                         "clicking a compact preview follows the visible page")
                keyboard.key("g", shift=True)
                expect(79, "G returns to current page after manual preview scrolling")
                wait_for(lambda: image_point(240), "the last preview and its image are visible again")
                keyboard.key("t")
                wait_for(lambda: not json.loads(config_path.read_text())["thumbnails"], "compact list mode")
                keyboard.key("c")
                keyboard.key("a", ctrl=True)
                for character in "001 chapter":
                    keyboard.key(character, delay=0.025)
                keyboard.key(0xFF0D)
                expect(0, "chapter focus remains usable after compact preview navigation")
                keyboard.key("t")
                wait_for(lambda: image_point(240), "preview shortcut reveals the first page after chapter navigation")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                print("PASS: responsive preview pixels below long contents/bookmarks, 240x240 layout, width/height resizing, rotation, inversion, manual-scroll anchors, actual image clicks, list switching and chapter focus")
            except Exception:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                if config_path.exists():
                    print(config_path.read_text(), file=sys.stderr)
                raise
            finally:
                if application.poll() is None:
                    application.terminate()
                    application.wait(timeout=5)
                keyboard.x11.XCloseDisplay(keyboard.display)


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
