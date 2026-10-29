#!/usr/bin/env python3
"""Integration checks for ready displays, parallel isolation and cleanup."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

WRAPPER = Path(__file__).with_name("with-xvfb.py")


def wait_for(predicate):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise AssertionError("Timed out waiting for the Xvfb wrapper probe")


def running(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


class XvfbWrapper(unittest.TestCase):
    def invoke(self, *arguments, **options):
        return subprocess.run([sys.executable, str(WRAPPER), *arguments],
                              text=True, capture_output=True, timeout=20, **options)

    def test_ready_display_is_connectable_and_preserves_arguments_and_status(self):
        probe = """
import ctypes, json, os, sys
x11 = ctypes.CDLL('libX11.so.6')
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
display = x11.XOpenDisplay(None)
assert display, 'the client must receive a ready, connectable X server'
x11.XCloseDisplay(display)
print(json.dumps(sys.argv[1:]))
sys.exit(7)
"""
        arguments = ["space in argument", "$(literal)", "semi;colon", "中文"]
        result = self.invoke(sys.executable, "-c", probe, *arguments)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertEqual(json.loads(result.stdout), arguments)

    def test_failed_server_never_launches_the_client(self):
        with tempfile.TemporaryDirectory(prefix="pagewing-xvfb-failure-") as directory:
            root = Path(directory)
            server = root / "Xvfb"
            server.write_text("#!/bin/sh\necho 'deliberate Xvfb failure' >&2\nexit 23\n")
            server.chmod(0o755)
            marker = root / "client-started"
            result = self.invoke(sys.executable, "-c",
                                 "from pathlib import Path; import sys; Path(sys.argv[1]).touch()",
                                 str(marker), env=dict(os.environ, PATH=str(root)))
            self.assertEqual(result.returncode, 1)
            self.assertIn("deliberate Xvfb failure", result.stderr)
            self.assertFalse(marker.exists())

    def test_sequential_clients_can_reconnect_after_the_last_client_closes(self):
        probe = """
import ctypes
x11 = ctypes.CDLL('libX11.so.6')
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
for index in range(50):
    display = x11.XOpenDisplay(None)
    assert display, f'the owned display must survive client reconnection {index}'
    x11.XCloseDisplay(display)
"""
        result = self.invoke(sys.executable, "-c", probe)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_parallel_clients_have_distinct_live_displays(self):
        with tempfile.TemporaryDirectory(prefix="pagewing-xvfb-parallel-") as directory:
            root = Path(directory)
            probe = """
import os, sys, time
from pathlib import Path
Path(sys.argv[1]).write_text(os.environ['DISPLAY'])
while not Path(sys.argv[2]).exists():
    time.sleep(0.02)
"""
            clients = []
            release = root / "release"
            try:
                for index in range(6):
                    clients.append(subprocess.Popen(
                        [sys.executable, str(WRAPPER), sys.executable, "-c", probe,
                         str(root / str(index)), str(release)],
                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                    ))
                wait_for(lambda: all((root / str(index)).exists() for index in range(6)))
                displays = [(root / str(index)).read_text() for index in range(6)]
                self.assertEqual(len(set(displays)), 6, displays)
                release.touch()
                for client in clients:
                    _, error = client.communicate(timeout=8)
                    self.assertEqual(client.returncode, 0, error)
            finally:
                release.touch(exist_ok=True)
                for client in clients:
                    if client.poll() is None:
                        client.terminate()
                    client.communicate(timeout=8)

    def test_termination_reaps_the_client_and_server(self):
        with tempfile.TemporaryDirectory(prefix="pagewing-xvfb-cleanup-") as directory:
            state = Path(directory) / "processes.json"
            probe = """
import json, os, sys, time
from pathlib import Path
parent = os.getppid()
children = Path(f'/proc/{parent}/task/{parent}/children').read_text().split()
server = next(int(pid) for pid in children if int(pid) != os.getpid())
Path(sys.argv[1]).write_text(json.dumps([os.getpid(), server]))
time.sleep(60)
"""
            client = subprocess.Popen([sys.executable, str(WRAPPER), sys.executable,
                                       "-c", probe, str(state)],
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                def ready():
                    if client.poll() is not None:
                        raise AssertionError(client.communicate()[1])
                    return state.exists()

                wait_for(ready)
                processes = json.loads(state.read_text())
                self.assertTrue(all(running(pid) for pid in processes))
                client.send_signal(signal.SIGTERM)
                _, error = client.communicate(timeout=8)
                self.assertEqual(client.returncode, 128 + signal.SIGTERM, error)
                self.assertTrue(all(not running(pid) for pid in processes), processes)
            finally:
                if client.poll() is None:
                    client.terminate()
                client.communicate(timeout=8)


if __name__ == "__main__":
    unittest.main()
