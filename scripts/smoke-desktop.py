#!/usr/bin/env python3
"""Exercise the real Linux desktop app. Run under xvfb-run, with libXtst installed."""
import ctypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


def wait_for(predicate, description, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError(f"Timed out: {description}")


class Keyboard:
    def __init__(self):
        self.x11 = ctypes.CDLL("libX11.so.6")
        self.xtst = ctypes.CDLL("libXtst.so.6")
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.display = self.x11.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError("No X11 display. Run with xvfb-run -a.")
        self.x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        self.x11.XDefaultRootWindow.restype = ctypes.c_ulong
        self.x11.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
        self.x11.XFetchName.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_char_p)]
        self.x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_uint
        self.x11.XFlush.argtypes = [ctypes.c_void_p]
        self.x11.XFree.argtypes = [ctypes.c_void_p]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]

    def focus(self):
        root, parent = ctypes.c_ulong(), ctypes.c_ulong()
        children, count = ctypes.POINTER(ctypes.c_ulong)(), ctypes.c_uint()
        self.x11.XQueryTree(self.display, self.x11.XDefaultRootWindow(self.display), ctypes.byref(root), ctypes.byref(parent), ctypes.byref(children), ctypes.byref(count))
        found = None
        try:
            for i in range(count.value):
                name = ctypes.c_char_p()
                if self.x11.XFetchName(self.display, children[i], ctypes.byref(name)) and name.value:
                    if b"folio-smoke.pdf" in name.value:
                        found = children[i]
                    self.x11.XFree(name)
        finally:
            if children:
                self.x11.XFree(children)
        if found:
            self.x11.XSetInputFocus(self.display, found, 1, 0)
            self.x11.XFlush(self.display)
        return found

    def key(self, symbol, shift=False):
        if isinstance(symbol, str):
            symbol = ord(symbol)
        shift_code = self.x11.XKeysymToKeycode(self.display, 0xFFE1)
        code = self.x11.XKeysymToKeycode(self.display, symbol)
        if shift:
            self.xtst.XTestFakeKeyEvent(self.display, shift_code, 1, 0)
        self.xtst.XTestFakeKeyEvent(self.display, code, 1, 0)
        self.xtst.XTestFakeKeyEvent(self.display, code, 0, 0)
        if shift:
            self.xtst.XTestFakeKeyEvent(self.display, shift_code, 0, 0)
        self.x11.XFlush(self.display)
        time.sleep(0.08)


def run(binary):
    with tempfile.TemporaryDirectory(prefix="folio-smoke-") as directory:
        root = Path(directory)
        fixture = Path(__file__).resolve().parent.parent / "assets/folio-guide.pdf"
        document = root / "阅读指南-folio-smoke.pdf"
        shutil.copyfile(fixture, document)
        environment = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), LIBGL_ALWAYS_SOFTWARE="1")
        config_path = root / "config/folio/library.json"
        keyboard = Keyboard()
        log = open(root / "application.log", "w+")
        application = None

        def saved(predicate):
            try:
                config = json.loads(config_path.read_text())
                return predicate(config, config["recent"][0])
            except (OSError, ValueError, KeyError, IndexError):
                return False

        def expect(predicate, description):
            wait_for(lambda: saved(predicate), description)

        def launch():
            process = subprocess.Popen([str(binary), str(document)], env=environment, stdout=log, stderr=log)
            wait_for(keyboard.focus, "reader window")
            return process

        try:
            application = launch()
            expect(lambda _, book: book["page"] == 0, "initial page")
            keyboard.key("f"); keyboard.key("a")
            expect(lambda _, book: book["page"] == 1, "keyboard link hints")
            keyboard.key("g"); keyboard.key("g")
            expect(lambda _, book: book["page"] == 0, "return from internal link")
            keyboard.key("3"); keyboard.key("g", shift=True)
            expect(lambda _, book: book["page"] == 2, "3G navigation")
            keyboard.key("b")
            expect(lambda _, book: 2 in book["bookmarks"], "saved bookmark")
            keyboard.key("w"); keyboard.key("+", shift=True)
            keyboard.key("2"); keyboard.key("j")
            keyboard.key("m"); keyboard.key("a")
            expect(lambda _, book: book["marks"].get("a", {}).get("page") == 2 and book["marks"]["a"]["offset"][1] > 0, "named position with scroll offset")
            keyboard.key("g", shift=True); keyboard.key("r"); keyboard.key("p")
            keyboard.key("'"); keyboard.key("a")
            expect(lambda _, book: book["page"] == 2 and book["rotation"] == 0 and isinstance(book["fit"], dict) and book["offset"][1] > 0, "mark restores page, zoom, rotation and scroll")
            keyboard.key("p")
            keyboard.key("g"); keyboard.key("g")
            expect(lambda _, book: book["page"] == 0, "gg navigation")
            keyboard.key("g", shift=True)
            expect(lambda _, book: book["page"] == 5, "G navigation")
            keyboard.key("r")
            expect(lambda _, book: book["rotation"] == 1, "page rotation")
            keyboard.key("d"); keyboard.key("i")
            expect(lambda config, _: config["dark"] and config["night"], "reading themes")
            keyboard.key("/")
            for letter in "native":
                keyboard.key(letter)
            time.sleep(0.5)
            keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 0 and book["rotation"] == 1, "full text search and typing focus")
            keyboard.key(0xFF1B)
            keyboard.key(":", shift=True); keyboard.key("4"); keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 3, "go to page dialog")
            keyboard.key(0xFF09)
            expect(lambda config, _: config["sidebar"], "document outline sidebar")
            keyboard.key("j", shift=True)
            expect(lambda _, book: book["page"] == 4, "Vim navigation after sidebar focus")
            keyboard.key("k", shift=True)
            expect(lambda _, book: book["page"] == 3, "return before session restore")
            application.terminate(); application.wait(timeout=5)
            application = launch()
            time.sleep(0.5)
            keyboard.key("j", shift=True)
            expect(lambda _, book: book["page"] == 4 and 2 in book["bookmarks"] and book["rotation"] == 1, "reading progress restored on restart")
            keyboard.key("'"); keyboard.key("a")
            expect(lambda _, book: book["page"] == 2 and book["rotation"] == 0 and book["offset"][1] > 0, "named position persisted after restart")
            application.terminate(); application.wait(timeout=5)
            application = launch()
            time.sleep(0.5)
            keyboard.key("m"); keyboard.key("z")
            expect(lambda _, book: book["marks"].get("z", {}).get("offset", [0, 0])[1] > 0, "page scroll restored on restart")
            print("PASS: native Vim navigation, keyboard links, bookmarks, named marks, scroll restore, rotation, themes, search, page dialog, sidebar and session restore")
        except Exception:
            log.flush(); log.seek(0)
            print(log.read(), file=sys.stderr)
            raise
        finally:
            if application and application.poll() is None:
                application.terminate(); application.wait(timeout=5)
            keyboard.x11.XCloseDisplay(keyboard.display)
            log.close()


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
