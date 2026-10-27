#!/usr/bin/env python3
"""Exercise image probing through real tmux after a terminal-mode exec handoff.

Run with JCODE_TEST_BINARY=/absolute/path/to/jcode and JCODE_TEST_SOCKET pointing
at a running daemon. No model request is sent. Requires tmux and a Unix PTY.
"""

import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tty
import unittest


REPLY = b"\x1b_Gi=31;OK\x1b\\\x1b[?62;4c\x1b[6;16;8t\x1b[0n"


def inherited_client(binary):
    tty.setraw(0)
    os.write(1, b"\x1b[>4;2m\x1b[>7u")
    os.environ.update(
        JCODE_RESUMING="1",
        JCODE_TUI_INHERITED_MODES="mouse=1,keyboard=1,focus=1",
        JCODE_TUI_INHERITED_THEME="dark",
    )
    os.execv(binary, [binary, "--no-update"])


@unittest.skipUnless(
    os.environ.get("JCODE_TEST_BINARY")
    and os.environ.get("JCODE_TEST_SOCKET")
    and shutil.which("tmux"),
    "requires JCODE_TEST_BINARY, JCODE_TEST_SOCKET, and tmux",
)
class TmuxProbeReload(unittest.TestCase):
    def test_inherited_keyboard_probe_preserves_input(self):
        for delivery in ("immediate", "duplicate", "fragmented"):
            with self.subTest(delivery=delivery):
                self.check_delivery(delivery)

    def check_delivery(self, delivery):
        binary = str(Path(os.environ["JCODE_TEST_BINARY"]).resolve())
        with tempfile.TemporaryDirectory(
            prefix="jcode-tmux-probe-", dir=os.environ.get("JCODE_SCRATCH_DIR")
        ) as directory:
            root = Path(directory)
            config = root / "tmux.conf"
            config.write_text(
                "set -g default-terminal tmux-256color\n"
                "set -g allow-passthrough on\n"
                "set -s escape-time 0\n"
                "set -s extended-keys on\n"
                "set -s extended-keys-format csi-u\n"
                "set -g status off\n"
            )
            command, response = root / "command", root / "response"
            socket = str(root / "tmux.sock")
            pid, fd = pty.fork()
            if pid == 0:
                env = os.environ.copy()
                env.pop("TMUX", None)
                env.pop("TMUX_PANE", None)
                env.update(
                    TERM="st-256color",
                    JCODE_HOME=directory,
                    JCODE_SOCKET=os.environ["JCODE_TEST_SOCKET"],
                    JCODE_THEME="dark",
                    JCODE_MERMAID_PICKER_PROBE="1",
                    JCODE_DEBUG_CMD_PATH=str(command),
                    JCODE_DEBUG_RESPONSE_PATH=str(response),
                )
                os.execvpe("tmux", [
                    "tmux", "-S", socket, "-f", str(config), "new-session",
                    "-s", "probe", "-x", "110", "-y", "35",
                    sys.executable, str(Path(__file__).resolve()), "--inner", binary,
                ], env)
            try:
                self.observe(fd, command, response, delivery)
            finally:
                subprocess.run(
                    ["tmux", "-S", socket, "kill-server"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    timeout=5, check=False,
                )
                try:
                    os.kill(pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                os.waitpid(pid, 0)
                os.close(fd)

    def observe(self, fd, command, response, delivery):
        deadline = time.monotonic() + 15
        captured = bytearray()
        probe_at = None
        first_sent = False
        extra_sent = False
        fragment = 0
        fragment_at = 0
        typed = False
        requested = False
        input_result = None
        expected = "user-keystrokes"
        while time.monotonic() < deadline:
            if select.select([fd], [], [], 0.02)[0]:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    break
                if not data:
                    break
                captured.extend(data)
                if probe_at is None and b"_Gi=31" in captured:
                    probe_at = time.monotonic()
            if probe_at is None:
                continue
            now = time.monotonic()
            age = now - probe_at
            if not first_sent and age > 0.05:
                os.write(fd, REPLY)
                first_sent = True
            if delivery == "duplicate" and age > 1 and not extra_sent:
                os.write(fd, REPLY + REPLY)
                extra_sent = True
            if delivery == "fragmented" and age > 0.5 and now - fragment_at > 0.04:
                if fragment < len(REPLY):
                    os.write(fd, REPLY[fragment:fragment + 1])
                    fragment += 1
                    fragment_at = now
                extra_sent = fragment == len(REPLY)
            ready = delivery == "immediate" or extra_sent
            if ready and age > 2.5 and not typed:
                os.write(fd, expected.encode())
                typed = True
            if typed and age > 3 and not requested:
                command.write_text("input\n")
                requested = True
            if requested and response.exists():
                if input_result is None:
                    input_result = response.read_text().strip()
                    response.unlink()
                    command.write_text("mermaid:stats\n")
                else:
                    stats = json.loads(response.read_text())
                    self.assertEqual(input_result, "input: " + json.dumps(expected))
                    self.assertEqual(stats.get("protocol"), "Kitty")
                    return
        self.fail(f"No client observation for {delivery}, probe seen: {probe_at is not None}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--inner":
        inherited_client(sys.argv[2])
    else:
        unittest.main()
