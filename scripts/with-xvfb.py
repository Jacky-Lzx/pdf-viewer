#!/usr/bin/env python3
"""Run a command on its own ready Xvfb display, including concurrent invocations."""
from contextlib import contextmanager
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import tempfile
import time


def stop(process):
    """Stop the owned process group, including clients left by a failed test."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        pass
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()


@contextmanager
def display():
    # Xvfb reserves the display itself before publishing its number. Scanning
    # /tmp/.X*-lock in a wrapper can assign the same display to concurrent tests.
    read_fd, write_fd = os.pipe()
    server = None
    with tempfile.TemporaryFile(mode="w+") as log:
        try:
            server = subprocess.Popen(
                ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1280x1024x24",
                 "-nolisten", "tcp"],
                pass_fds=(write_fd,), stdout=subprocess.DEVNULL, stderr=log,
                start_new_session=True,
            )
            os.close(write_fd)
            write_fd = None
            ready = bytearray()
            deadline = time.monotonic() + 15
            with selectors.DefaultSelector() as selector:
                selector.register(read_fd, selectors.EVENT_READ)
                while b"\n" not in ready:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not selector.select(remaining):
                        raise RuntimeError("Xvfb did not publish a ready display within 15 seconds")
                    chunk = os.read(read_fd, 32)
                    if not chunk:
                        raise RuntimeError("Xvfb exited before publishing a ready display")
                    ready.extend(chunk)
                    if len(ready) > 32:
                        raise RuntimeError("Xvfb published an invalid display number")
            number = ready.strip()
            if not number.isdigit() or server.poll() is not None:
                raise RuntimeError("Xvfb did not publish a valid live display")
            yield ":" + number.decode("ascii")
        except RuntimeError as error:
            log.seek(0)
            details = log.read().strip()
            raise RuntimeError(f"{error}\n{details}".rstrip()) from error
        finally:
            os.close(read_fd)
            if write_fd is not None:
                os.close(write_fd)
            if server is not None:
                stop(server)


def run(command):
    with display() as name:
        environment = dict(os.environ, DISPLAY=name)
        process = subprocess.Popen(command, env=environment, start_new_session=True)
        try:
            result = process.wait()
            return 128 - result if result < 0 else result
        finally:
            stop(process)


def main():
    if len(sys.argv) < 2 or sys.argv[1] in ("-h", "--help"):
        print(f"Usage: python3 {Path(__file__).name} COMMAND [ARG ...]")
        return 2 if len(sys.argv) < 2 else 0
    # A terminated wrapper must still tear down its client and X server.
    signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))
    try:
        return run(sys.argv[1:])
    except KeyboardInterrupt:
        return 130
    except (OSError, RuntimeError) as error:
        print(f"with-xvfb: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
