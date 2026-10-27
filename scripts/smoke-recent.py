#!/usr/bin/env python3
"""Check native recent-document filtering, keyboard focus and selection."""
import ctypes
import json
import os
from pathlib import Path
import runpy
import signal
import shutil
import subprocess
import sys
import tempfile
import time

SCRIPTS = Path(__file__).resolve().parent
desktop = runpy.run_path(str(SCRIPTS / "smoke-desktop.py"))
Keyboard, wait_for = desktop["Keyboard"], desktop["wait_for"]


def run(binary):
    with tempfile.TemporaryDirectory(prefix="pagewing-recent-") as directory:
        root = Path(directory)
        paths = [root / "alpha.pdf", root / "manuals/plan-a.pdf", root / "manuals/plan-z.pdf"]
        paths += [root / f"archive/document-{index}.pdf" for index in range(21)]
        for path in paths:
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SCRIPTS.parent / "assets/pagewing-guide.pdf", path)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/pagewing/library.json"
        config_path.parent.mkdir(parents=True)
        config_path.write_text(json.dumps({"recent": [{"path": str(path), "page": index % 6} for index, path in enumerate(paths)]}))
        keyboard = Keyboard()
        with open(root / "application.log", "w+") as log:
            application = subprocess.Popen([str(binary), str(paths[0])], env=environment, stdout=log, stderr=log)

            def expect(path, page, description):
                def saved():
                    try:
                        book = json.loads(config_path.read_text())["recent"][0]
                        return book["path"] == str(path) and book["page"] == page
                    except (OSError, ValueError, KeyError, IndexError):
                        return False
                wait_for(saved, description)

            def picker(query="", burst=False):
                keyboard.key("p", ctrl=True)
                for character in query:
                    keyboard.key(character, delay=0 if burst else 0.025)

            def burst_motion(keys):
                application.send_signal(signal.SIGSTOP)
                try:
                    for key in keys:
                        keyboard.key(key, ctrl=True, delay=0)
                    keyboard.key(0xFF0D, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                time.sleep(0.4)

            def selected_row_visible(minimum=False):
                for y in (range(100, 184, 2) if minimum else range(150, 480, 2)):
                    colors = [keyboard.pixel((x, y)) for x in ((40, 100, 170) if minimum else (200, 400, 600))]
                    if all(green > red + 8 and green > blue + 6 for red, green, blue in colors):
                        return True
                return False

            try:
                window = wait_for(lambda: keyboard.focus("alpha.pdf"), "recent picker reader window")
                expect(paths[0], 0, "initial document")
                picker("manuals plan")
                burst_motion(["k", "j"])
                expect(paths[2], 2, "same-frame up at the top then down reaches the second result")
                picker("alpha"); keyboard.key(0xFF0D)
                expect(paths[0], 0, "reopen an unrelated document before the second boundary check")
                picker("manuals plan")
                burst_motion(["j", "j", "k"])
                expect(paths[2], 2, "same-frame down at the bottom then up returns to the first result")
                # The final distinguishing letter and Enter can arrive in one frame.
                picker("manuals plan-z", burst=True)
                keyboard.key(0xFF0D)
                expect(paths[2], 2, "burst filtering opens plan-z and restores its saved page")
                picker("manuals plan")
                keyboard.key(0xFF54); keyboard.key(0xFF0D)
                expect(paths[1], 1, "ArrowDown chooses plan-a after recent order changes")
                picker("manuals plan")
                keyboard.key("j", ctrl=True); keyboard.key("k", ctrl=True); keyboard.key("j", ctrl=True)
                keyboard.key(0xFF0D)
                expect(paths[2], 2, "Ctrl+J/K move results without inserting text")
                picker()
                keyboard.key("p", ctrl=True)
                keyboard.key("j", shift=True)
                expect(paths[2], 3, "shortcut toggles picker closed and releases keyboard navigation")
                keyboard.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
                keyboard.x11.XResizeWindow(keyboard.display, window, 860, 540)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                picker()
                for _ in range(23):
                    keyboard.key("j", ctrl=True, delay=0.015)
                wait_for(selected_row_visible, "last selected recent document is drawn in the compact window")
                keyboard.key(0xFF0D)
                expect(paths[-1], 5, "last of 24 documents opens with its saved reading state")
                picker("jkgmissing")
                keyboard.key(0xFF0D)
                time.sleep(0.3)
                keyboard.key(0xFF1B)
                keyboard.key("k", shift=True)
                expect(paths[-1], 4, "empty results do not navigate or open a stale document; Escape restores navigation")

                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                picker()
                for _ in range(23):
                    keyboard.key("j", ctrl=True, delay=0.015)
                wait_for(lambda: selected_row_visible(minimum=True), "last result stays visible at minimum size")
                keyboard.key(0xFF0D)
                expect(paths[-2], 4, "last recent result opens at minimum size")
                picker("alpha")
                # One result: Tab goes to the row, Open, then Cancel.
                for _ in range(3):
                    keyboard.key(0xFF09)
                keyboard.key(0xFF0D)
                keyboard.key("k", shift=True)
                expect(paths[-2], 3, "Enter on Cancel closes the picker without opening its selected result")
                picker("alpha")
                keyboard.key(0xFF09); keyboard.key(0xFF09); keyboard.key(0xFF0D)
                expect(paths[0], 0, "Tab and Enter activate Open at minimum size")
                picker("jkgmissing")
                keyboard.key(0xFF09); keyboard.key(0xFF0D)
                keyboard.key("j", shift=True)
                expect(paths[0], 1, "empty results leave Cancel accessible with Tab and Enter")
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                # Empty history must also remain usable from the welcome screen.
                config_path.write_text('{"recent": []}')
                application = subprocess.Popen([str(binary)], env=environment, stdout=log, stderr=log)
                window = wait_for(lambda: keyboard.focus("Pagewing"), "welcome window without recent documents")
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                picker(); keyboard.key(0xFF0D); keyboard.key(0xFF09); keyboard.key(0xFF0D)
                keyboard.key("q", ctrl=True)
                assert application.wait(timeout=8) == 0
                assert not json.loads(config_path.read_text())["recent"]
                print("PASS: burst/token filtering, ordered same-frame movement at both edges, saved-page restore, arrows and Ctrl+J/K, toggle/Escape, 24-row compact/minimum-size scrolling, Tab/Enter Open/Cancel, empty matches/history and graceful quit")
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
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/pagewing").resolve())
