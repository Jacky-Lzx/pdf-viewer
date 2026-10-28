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
        keyboard.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        keyboard.xtst.XTestFakeMotionEvent(keyboard.display, -1, 10, 10, 0)
        keyboard.x11.XFlush(keyboard.display)
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

            def navigate_and_open(motions, index):
                # Snapshot the current MRU order because every open changes it.
                target = json.loads(config_path.read_text())["recent"][index]
                picker()
                application.send_signal(signal.SIGSTOP)
                try:
                    for key, control in motions:
                        keyboard.key(key, ctrl=control, delay=0)
                    keyboard.key(0xFF0D, delay=0)
                finally:
                    application.send_signal(signal.SIGCONT)
                expect(Path(target["path"]), target["page"],
                       f"ordered page/boundary navigation opens result {index + 1}")

            def selected_rows(size=(860, 540)):
                width, height = size
                gap = min(80, (width - 64) // 4)
                rows = []
                for y in range(80, height - 40, 2):
                    colors = [keyboard.pixel((x, y)) for x in (width // 2 - gap, width // 2, width // 2 + gap)]
                    if all(green > red + 8 and green > blue + 6 for red, green, blue in colors):
                        rows.append(y)
                # Locate one 32-pixel row. Ignore isolated title/input text,
                # while allowing label glyphs to interrupt its background fill.
                return max(
                    ([row for row in rows if y <= row < y + 32] for y in rows),
                    key=len,
                    default=[],
                )

            def selected_row_visible(size=(860, 540)):
                # Require a broad fill on several scanlines, excluding thin
                # focus borders and green text in the input or title.
                return len(selected_rows(size)) >= 6

            try:
                window = wait_for(lambda: keyboard.focus("alpha.pdf"), "recent picker reader window")
                expect(paths[0], 0, "initial document")
                picker("manuals plan")
                for edge, arrow in [(0xFF57, 0xFF52), (0xFF50, 0xFF54)]:
                    keyboard.key(edge)
                    keyboard.key(arrow, shift=True)
                    keyboard.key("c", ctrl=True)
                    assert keyboard.clipboard() == "manuals plan", "Shift+arrows select filter text"
                keyboard.key(0xFF57)
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
                for _ in range(12):
                    keyboard.key("j", ctrl=True, delay=0.015)
                wait_for(selected_row_visible, "middle recent result is visible")
                time.sleep(0.3)
                before_rows = selected_rows()
                before_y = sum(before_rows) / len(before_rows)
                keyboard.key(0xFF52)

                def visible_neighbor_kept_its_position():
                    rows = selected_rows()
                    return rows and abs(sum(rows) / len(rows) - (before_y - 40)) <= 3

                wait_for(visible_neighbor_kept_its_position,
                         "ArrowUp selects the visible preceding row without recentering the list")
                keyboard.key(0xFF54)

                def returned_to_the_same_row_position():
                    rows = selected_rows()
                    return rows and abs(sum(rows) / len(rows) - before_y) <= 3

                wait_for(returned_to_the_same_row_position,
                         "ArrowDown returns to the same row position without moving the list")
                for _ in range(23):
                    keyboard.key("j", ctrl=True, delay=0.015)
                wait_for(selected_row_visible, "last selected recent document is drawn in the compact window")
                for size in ((1180, 820), (860, 240), (240, 240)):
                    keyboard.x11.XResizeWindow(keyboard.display, window, *size)
                    keyboard.x11.XFlush(keyboard.display)
                    time.sleep(0.2)
                    wait_for(lambda: selected_row_visible(size), f"selected result remains visible after resize to {size}")
                rows = selected_rows((240, 240))
                keyboard.wheel((120, rows[len(rows) // 2]), down=False, steps=20)
                wait_for(lambda: not selected_row_visible((240, 240)), "manual scrolling can hide the selected result")
                time.sleep(0.3)
                assert not selected_row_visible((240, 240)), "ordinary redraw must preserve manual scroll"
                for size in ((240, 400), (240, 240), (860, 540)):
                    keyboard.x11.XResizeWindow(keyboard.display, window, *size)
                    keyboard.x11.XFlush(keyboard.display)
                    time.sleep(0.2)
                    wait_for(lambda: selected_row_visible(size), f"resizing reveals the retained selection at {size}")
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
                wait_for(lambda: selected_row_visible((240, 240)), "last result stays visible at minimum size")
                keyboard.key(0xFF0D)
                expect(paths[-2], 4, "last recent result opens at minimum size")
                picker("alpha")
                # One result: row, remove, Open, Copy, then Cancel.
                for _ in range(4):
                    keyboard.key(0xFF09)
                keyboard.key(0xFF0D)
                assert keyboard.clipboard() == str(paths[0]), "Copy keeps the full selected path at minimum size"
                keyboard.key(0xFF09); keyboard.key(0xFF0D)
                keyboard.key("k", shift=True)
                expect(paths[-2], 3, "Enter on Cancel closes the picker without opening its selected result")
                picker("alpha")
                keyboard.key(0xFF09); keyboard.key(0xFF09); keyboard.key(0xFF09); keyboard.key(0xFF0D)
                expect(paths[0], 0, "Tab and Enter activate Open at minimum size")
                picker("jkgmissing")
                keyboard.key(0xFF57, ctrl=True)  # Ctrl+End with no results
                keyboard.key(0xFF55); keyboard.key(0xFF56)
                keyboard.key(0xFF09); keyboard.key(0xFF0D)
                keyboard.key("j", shift=True)
                expect(paths[0], 1, "empty results leave Cancel accessible with Tab and Enter")

                keyboard.x11.XResizeWindow(keyboard.display, window, 1180, 820)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                navigate_and_open([(0xFF56, False)], 7)  # PageDown: seven visible rows
                navigate_and_open([(0xFF57, True), (0xFF56, False), (0xFF52, False)], 22)
                navigate_and_open([(0xFF57, True), (0xFF50, True), (0xFF55, False), (0xFF54, False)], 1)
                keyboard.x11.XResizeWindow(keyboard.display, window, 240, 240)
                keyboard.x11.XFlush(keyboard.display)
                time.sleep(0.3)
                navigate_and_open([(0xFF56, False)], 1)  # One visible row at minimum size
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
                print("PASS: burst/token filtering, Shift+arrow text selection, ordered same-frame movement at both edges, saved-page restore, arrows and Ctrl+J/K, viewport-sized PageUp/PageDown, Ctrl+Home/End, stable visible neighbors, toggle/Escape, selected row after tall/short/narrow resizing, manual scroll preservation, 24-row compact/minimum-size scrolling, Tab/Enter Open/Copy/Cancel, full path clipboard without opening its document, empty matches/history and graceful quit")
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
