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

    def key(self, symbol, shift=False, ctrl=False, delay=0.08):
        if isinstance(symbol, str):
            symbol = ord(symbol)
        shift_code = self.x11.XKeysymToKeycode(self.display, 0xFFE1)
        ctrl_code = self.x11.XKeysymToKeycode(self.display, 0xFFE3)
        code = self.x11.XKeysymToKeycode(self.display, symbol)
        if ctrl:
            self.xtst.XTestFakeKeyEvent(self.display, ctrl_code, 1, 0)
        if shift:
            self.xtst.XTestFakeKeyEvent(self.display, shift_code, 1, 0)
        self.xtst.XTestFakeKeyEvent(self.display, code, 1, 0)
        self.xtst.XTestFakeKeyEvent(self.display, code, 0, 0)
        if shift:
            self.xtst.XTestFakeKeyEvent(self.display, shift_code, 0, 0)
        if ctrl:
            self.xtst.XTestFakeKeyEvent(self.display, ctrl_code, 0, 0)
        self.x11.XFlush(self.display)
        time.sleep(delay)

    def drag(self, start, end):
        self.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeMotionEvent(self.display, -1, *start, 0)
        self.xtst.XTestFakeButtonEvent(self.display, 1, 1, 0)
        self.x11.XFlush(self.display)
        time.sleep(0.1)
        for step in range(1, 11):
            x, y = [round(a + (b - a) * step / 10) for a, b in zip(start, end)]
            self.xtst.XTestFakeMotionEvent(self.display, -1, x, y, 0)
            self.x11.XFlush(self.display)
            time.sleep(0.03)
        self.xtst.XTestFakeButtonEvent(self.display, 1, 0, 0)
        self.x11.XFlush(self.display)
        time.sleep(0.1)

    def wheel(self, position, ctrl=False):
        self.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        ctrl_code = self.x11.XKeysymToKeycode(self.display, 0xFFE3)
        self.xtst.XTestFakeMotionEvent(self.display, -1, *position, 0)
        if ctrl:
            self.xtst.XTestFakeKeyEvent(self.display, ctrl_code, 1, 0)
        self.xtst.XTestFakeButtonEvent(self.display, 4, 1, 0)
        self.xtst.XTestFakeButtonEvent(self.display, 4, 0, 0)
        if ctrl:
            self.xtst.XTestFakeKeyEvent(self.display, ctrl_code, 0, 0)
        self.x11.XFlush(self.display)
        time.sleep(0.2)

    def clipboard(self):
        x11 = self.x11
        x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
        x11.XInternAtom.restype = ctypes.c_ulong
        x11.XCreateSimpleWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong, ctypes.c_ulong]
        x11.XCreateSimpleWindow.restype = ctypes.c_ulong
        x11.XConvertSelection.argtypes = [ctypes.c_void_p] + [ctypes.c_ulong] * 5
        x11.XGetWindowProperty.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_long, ctypes.c_long, ctypes.c_int, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_void_p)]
        x11.XDestroyWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        window = x11.XCreateSimpleWindow(self.display, x11.XDefaultRootWindow(self.display), 0, 0, 1, 1, 0, 0, 0)
        atom = lambda name: x11.XInternAtom(self.display, name, 0)
        property_atom = atom(b"FOLIO_SMOKE_CLIPBOARD")
        try:
            x11.XConvertSelection(self.display, atom(b"CLIPBOARD"), atom(b"UTF8_STRING"), property_atom, window, 0)
            x11.XFlush(self.display)
            def read():
                actual, count, remaining = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_ulong()
                format_bits, data = ctypes.c_int(), ctypes.c_void_p()
                x11.XGetWindowProperty(self.display, window, property_atom, 0, 65536, 0, 0, ctypes.byref(actual), ctypes.byref(format_bits), ctypes.byref(count), ctypes.byref(remaining), ctypes.byref(data))
                try:
                    if actual.value and format_bits.value == 8 and data.value:
                        return ctypes.string_at(data, count.value).decode("utf-8")
                finally:
                    if data.value:
                        x11.XFree(data)
                return None
            return wait_for(read, "selected text in clipboard")
        finally:
            x11.XDestroyWindow(self.display, window)


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
            keyboard.key("v"); time.sleep(0.3)
            keyboard.drag((374, 290), (570, 290)); keyboard.key("y")
            selected = keyboard.clipboard()
            assert selected == "A native PDF reader, written in Rust.", repr(selected)
            keyboard.key("r"); time.sleep(0.3)
            keyboard.drag((768, 135), (768, 424)); keyboard.key("c", ctrl=True)
            selected = keyboard.clipboard()
            assert selected == "A native PDF reader, written in Rust.", repr(selected)
            keyboard.key(0xFF1B); keyboard.key("r", shift=True)
            keyboard.key("+", shift=True)
            expect(lambda _, book: 0.49 < book["offset"][1] < 0.51, "keyboard zoom preserves the reading center")
            keyboard.key("p")
            keyboard.wheel((590, 650), ctrl=True)
            expect(lambda _, book: 0.80 < book["offset"][1] < 0.86, "Ctrl+wheel zoom preserves the pointer position")
            keyboard.key("p")
            keyboard.key("w")
            for _ in range(8): keyboard.key("+", shift=True, delay=0.015)
            for letter in "jjjkkkjjk": keyboard.key(letter, shift=True, delay=0.015)
            keyboard.key("g"); keyboard.key("g"); keyboard.key("p")
            expect(lambda _, book: book["page"] == 0 and book["fit"] == "Page", "rapid zoom and page changes settle on the latest view")
            keyboard.key("f"); keyboard.key("a")
            expect(lambda _, book: book["page"] == 1, "keyboard link hints")
            keyboard.key("g"); keyboard.key("g")
            expect(lambda _, book: book["page"] == 0, "return from internal link")
            keyboard.key(":", shift=True); keyboard.key("i"); keyboard.key("i"); keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 1, "Roman PDF page label")
            keyboard.key(":", shift=True); keyboard.key("="); keyboard.key("1"); keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 2, "explicit numeric PDF label")
            keyboard.key(":", shift=True); keyboard.key("a", shift=True); keyboard.key("-"); keyboard.key("2"); keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 5, "prefixed PDF page label")
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
            updated_document = document.read_bytes().replace(b"little", b"gentle")
            document.write_bytes(updated_document)
            keyboard.key("r", ctrl=True)
            time.sleep(0.3)
            keyboard.key("m"); keyboard.key("q")
            expect(lambda _, book: book["marks"].get("q", {}).get("page") == 2 and book["marks"]["q"]["offset"][1] > 0, "reload preserves reading position and marks")
            keyboard.key("/"); keyboard.key("a", ctrl=True)
            for letter in "gentle": keyboard.key(letter)
            keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 0, "reload exposes updated document text")
            keyboard.key(0xFF1B)
            document.write_bytes(b"incomplete update")
            keyboard.key("r", ctrl=True); time.sleep(0.3)
            keyboard.key("j", shift=True); keyboard.key("y")
            assert "NAVIGATION" in keyboard.clipboard(), "failed reload should preserve the previous PDF"
            document.write_bytes(updated_document)
            keyboard.key("r", ctrl=True); time.sleep(0.3)
            keyboard.key("'"); keyboard.key("a")
            expect(lambda _, book: book["page"] == 2 and book["offset"][1] > 0, "reload recovers after an incomplete file update")
            keyboard.key("g", shift=True)
            expect(lambda _, book: book["page"] == 5, "last page before document shrinks")
            kids = b"/Kids [5 0 R 7 0 R 9 0 R 11 0 R 13 0 R 15 0 R]"
            shortened = updated_document.replace(kids, b"/Kids [5 0 R 7 0 R 9 0 R 11 0 R]".ljust(len(kids), b" "), 1).replace(b"/Count 6", b"/Count 4", 1)
            document.write_bytes(shortened)
            keyboard.key("r", ctrl=True)
            expect(lambda _, book: book["page"] == 3, "reload clamps the page when the PDF shrinks")
            keyboard.key("y")
            assert "SEARCH AND BOOKMARKS" in keyboard.clipboard(), "expected the last remaining page"
            document.write_bytes(updated_document)
            keyboard.key("r", ctrl=True); time.sleep(0.3)
            keyboard.key("'"); keyboard.key("a")
            expect(lambda _, book: book["page"] == 2 and book["offset"][1] > 0, "valid marks survive document resizing")
            keyboard.key("p")
            keyboard.key("g"); keyboard.key("g")
            expect(lambda _, book: book["page"] == 0, "gg navigation")
            keyboard.key("g", shift=True)
            expect(lambda _, book: book["page"] == 5, "G navigation")
            keyboard.key("r")
            expect(lambda _, book: book["rotation"] == 1, "page rotation")
            keyboard.key("d"); keyboard.key("i")
            expect(lambda config, _: config["dark"] and config["night"], "reading themes")
            keyboard.key("w"); keyboard.key("+", shift=True)
            keyboard.key("/")
            keyboard.key("a", ctrl=True)
            for letter in "native":
                keyboard.key(letter)
            keyboard.key(0xFF0D)
            expect(lambda _, book: book["page"] == 0 and book["rotation"] == 1 and book["offset"][0] > 0, "rotated zoomed search scrolls to the match without stealing typing focus")
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
            print("PASS: native Vim navigation, links, text selection, clipboard, bookmarks, marks, zoom anchors, reload and recovery, PDF labels, search, sidebar and session restore")
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
            log.close()


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/folio").resolve())
