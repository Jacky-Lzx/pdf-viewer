#!/usr/bin/env python3
"""Check native screen snapshots, field locators and geometry synchronization."""
import ctypes
from pathlib import Path
import runpy
import select
import subprocess
import sys
import tempfile
import unittest

desktop = runpy.run_path(str(Path(__file__).with_name("smoke-desktop.py")))
Keyboard, assert_stays = desktop["Keyboard"], desktop["assert_stays"]


class DesktopHelpers(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Keep a client alive between cases so Xvfb does not reset the display.
        cls.keyboard = Keyboard()
        cls.x11 = cls.keyboard.x11

    @classmethod
    def tearDownClass(cls):
        cls.x11.XCloseDisplay(cls.keyboard.display)

    def setUp(self):
        x11 = self.x11
        x11.XCreateSimpleWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                                          ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint,
                                          ctypes.c_uint, ctypes.c_ulong, ctypes.c_ulong]
        x11.XCreateSimpleWindow.restype = ctypes.c_ulong
        x11.XMapRaised.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        x11.XDestroyWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        x11.XCreateGC.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_void_p]
        x11.XCreateGC.restype = ctypes.c_void_p
        x11.XFreeGC.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
        x11.XSetForeground.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
        x11.XFillRectangle.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p,
                                      ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint]
        self.window = x11.XCreateSimpleWindow(
            self.keyboard.display, x11.XDefaultRootWindow(self.keyboard.display),
            20, 20, 160, 100, 0, 0, 0xFFFFFF)
        self.gc = x11.XCreateGC(self.keyboard.display, self.window, 0, None)
        x11.XMapRaised(self.keyboard.display, self.window)
        x11.XSync(self.keyboard.display, False)

    def tearDown(self):
        self.x11.XFreeGC(self.keyboard.display, self.gc)
        self.x11.XDestroyWindow(self.keyboard.display, self.window)

    def paint(self, bounds, color):
        self.x11.XSetForeground(self.keyboard.display, self.gc, color)
        self.x11.XFillRectangle(self.keyboard.display, self.window, self.gc, *bounds)
        self.x11.XSync(self.keyboard.display, False)

    def test_snapshot_stays_coherent_when_the_window_repaints(self):
        self.paint((0, 0, 160, 100), 0xAA2233)
        with self.keyboard.frame((20, 20, 160, 100)) as frame:
            self.paint((0, 0, 160, 100), 0x3344BB)
            self.assertEqual(frame.pixel((25, 25)), (170, 34, 51))
            self.assertEqual(frame.pixel((175, 115)), (170, 34, 51))
            self.assertEqual(self.keyboard.pixel((25, 25)), (51, 68, 187))
            with self.assertRaises(ValueError):
                frame.pixel((19, 25))
        with self.assertRaisesRegex(RuntimeError, "released"):
            frame.pixel((25, 25))

    def test_bands_and_field_centers_follow_actual_painted_bounds(self):
        self.paint((10, 5, 130, 1), 0xEEEEEE)  # A border cannot masquerade as a field.
        self.paint((10, 20, 130, 20), 0xEEEEEE)
        self.paint((10, 80, 130, 20), 0xEEEEEE)  # Includes a band at the capture edge.
        with self.keyboard.frame((20, 20, 160, 100)) as frame:
            self.assertEqual(frame.bands([60, 140], lambda rgb: rgb == (238, 238, 238),
                                         min_height=14, max_height=28), [(40, 60), (100, 120)])
            with self.assertRaises(ValueError):
                frame.bands([], lambda rgb: True)
        self.assertEqual(self.keyboard.input_position((80, 30, 90, 80)), (150, 49))
        self.paint((0, 0, 160, 100), 0xFFFFFF)
        self.paint((10, 35, 130, 26), 0xEEEEEE)
        self.assertEqual(self.keyboard.input_position((80, 30, 90, 80)), (150, 67))

    def test_missing_fields_and_empty_capture_bounds_are_rejected(self):
        self.assertIsNone(self.keyboard.input_position((80, 30, 90, 80)))
        with self.assertRaises(ValueError):
            with self.keyboard.frame((20, 20, 0, 100)):
                pass

    def test_resize_waits_for_the_requested_server_geometry(self):
        for width, height in [(240, 240), (400, 300), (240, 240)]:
            self.keyboard.resize(self.window, width, height)
            self.assertEqual(self.keyboard.size(self.window), (width, height))

    def test_reader_resize_waits_for_client_paint_after_server_geometry(self):
        self.x11.XMoveWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int]
        self.x11.XMoveWindow(self.keyboard.display, self.window, 0, 0)
        self.x11.XSync(self.keyboard.display, False)
        # A separate X11 client repaints a footer only after ConfigureNotify.
        # A geometry-only wait would return before this client's paint arrives.
        painter = r'''
import ctypes, runpy, sys, time
from pathlib import Path
desktop = runpy.run_path(sys.argv[1])
keyboard = desktop['Keyboard']()
x11, display, window = keyboard.x11, keyboard.display, int(sys.argv[2])
x11.XSelectInput.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_long]
x11.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
x11.XCreateGC.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_void_p]
x11.XCreateGC.restype = ctypes.c_void_p
x11.XSetForeground.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
x11.XFillRectangle.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p,
                              ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint]
gc = x11.XCreateGC(display, window, 0, None)
stamp = Path(sys.argv[3])
def paint():
    width, height = keyboard.size(window)
    temporary = stamp.with_suffix('.next')
    temporary.write_text(f'{width}x{height}')
    temporary.replace(stamp)
    x11.XSetForeground(display, gc, 0xFFFFFF)
    x11.XFillRectangle(display, window, gc, 0, 0, width, height)
    x11.XSetForeground(display, gc, 0x228855)
    x11.XFillRectangle(display, window, gc, 12, height - 24, 30, 8)
    x11.XSync(display, False)
x11.XSelectInput(display, window, (1 << 17) | (1 << 15))
paint()
print('ready', flush=True)
event = (ctypes.c_long * 24)()
while True:
    x11.XNextEvent(display, ctypes.byref(event))
    if ctypes.cast(event, ctypes.POINTER(ctypes.c_int))[0] in (12, 22):
        time.sleep(0.08)  # Deliberately make server geometry precede client paint.
        paint()
'''
        with tempfile.TemporaryDirectory(prefix="pagewing-repaint-probe-") as directory:
            stamp = Path(directory) / "painted"
            client = subprocess.Popen(
                [sys.executable, "-c", painter, str(Path(__file__).with_name("smoke-desktop.py")),
                 str(self.window), str(stamp)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                self.assertTrue(select.select([client.stdout], [], [], 5)[0], "painter did not start")
                self.assertEqual(client.stdout.readline().strip(), "ready")
                for width, height in [(240, 240), (400, 240), (400, 300), (240, 300), (240, 240)]:
                    self.keyboard.resize_reader(self.window, width, height)
                    self.assertEqual(self.keyboard.pixel((14, height - 22)), (34, 136, 85))
                    self.assertEqual(stamp.read_text(), f"{width}x{height}")
            finally:
                client.terminate()
                client.communicate(timeout=5)


class ObservationWindow(unittest.TestCase):
    def test_a_transient_invariant_failure_is_not_hidden_by_a_final_recovery(self):
        values = iter([True, False, True])
        with self.assertRaisesRegex(AssertionError, "lost selection"):
            assert_stays(lambda: next(values), "lost selection", duration=0.02)


if __name__ == "__main__":
    unittest.main()
