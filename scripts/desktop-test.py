#!/usr/bin/env python3
"""Graphical desktop acceptance: boots with a display, drives the desktop with
QMP mouse and keyboard input, and checks serial markers, screenshots and
persistence. Screenshots are kept in the output directory as evidence."""
import argparse
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import time
import zlib

# Start menu and desktop layout (desktop/src/main.rs), in logical pixels. The taskbar
# and start menu are anchored to the bottom of the screen, so their y coordinates are
# offsets from it. SCREEN is the logical size and SCALE the HiDPI factor, both set
# from the desktop's DESKTOP_READY line.
SCREEN = (1920, 1080)
SCALE = 1
START_FROM_BOTTOM = 23
MENU_FROM_BOTTOM = {"files": 452, "editor": 412, "terminal": 372, "monitor": 332, "calculator": 292, "notepad": 252,
                    "about": 212, "sync": 164, "exit": 124, "restart": 84}
TITLE_ACTIVE = (232, 236, 244)


def read_ppm(path):
    data = path.read_bytes()
    parts = data.split(maxsplit=4)
    assert parts[0] == b"P6", "screendump is not a binary PPM"
    width, height = int(parts[1]), int(parts[2])
    return width, height, parts[4][-width * height * 3:]


def write_png(path, width, height, rgb):
    rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    def chunk(kind, body):
        return len(body).to_bytes(4, "big") + kind + body + zlib.crc32(kind + body).to_bytes(4, "big")
    header = width.to_bytes(4, "big") + height.to_bytes(4, "big") + bytes([8, 2, 0, 0, 0])
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b""))


def vga_memory_mb(width, height):
    mb = 16
    while mb * 1024 * 1024 < width * height * 4: mb *= 2
    return mb


class Shot:
    """A screendump; `pixel` takes logical coordinates and samples the middle of the
    physical pixels that logical pixel covers."""
    def __init__(self, width, height, rgb): self.width, self.height, self.rgb = width, height, rgb
    def raw(self, x, y):
        """The pixel at unscaled screen coordinates."""
        i = (y * self.width + x) * 3
        return tuple(self.rgb[i:i + 3])
    def pixel(self, x, y):
        x, y = x * SCALE + SCALE // 2, y * SCALE + SCALE // 2
        i = (y * self.width + x) * 3
        return tuple(self.rgb[i:i + 3])
    def near(self, x, y, color, tolerance=10):
        return all(abs(a - b) <= tolerance for a, b in zip(self.pixel(x, y), color))


class Desktop:
    def __init__(self, source, output, disk, accel, memory, resolution):
        self.output = output
        output.mkdir(parents=True)
        # A short private directory for the QMP socket (Unix socket paths are
        # limited to 108 bytes, and output folders can be deep).
        self.control = tempfile.TemporaryDirectory(prefix="atom-qmp-")
        self.qmp_path = Path(self.control.name) / "qmp.sock"
        image = source / "target/x86_64-os/release/bootimage-x86_64-kernel.bin"
        command = ["qemu-system-x86_64", "-accel", accel, "-m", memory, "-smp", "1",
                   "-drive", f"format=raw,file={image},snapshot=on",
                   "-drive", f"if=none,format=raw,file={disk},id=atomdata,cache=writeback",
                   "-device", "virtio-blk-pci,drive=atomdata,disable-modern=on",
                   "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
                   "-device", f"VGA,xres={resolution[0]},yres={resolution[1]},vgamem_mb={vga_memory_mb(*resolution)}",
                   "-display", "none", "-nic", "none", "-serial", f"file:{output / 'serial.log'}",
                   "-qmp", f"unix:{self.qmp_path},server=on,wait=off", "-no-shutdown"]
        if accel == "kvm" and not os.access("/dev/kvm", os.R_OK | os.W_OK):
            command = ["sudo", "-n", "setpriv", f"--reuid={os.getuid()}", "--regid=kvm", "--init-groups"] + command
        (output / "command.json").write_text(json.dumps(command, indent=2) + "\n")
        self.errors = (output / "qemu.stderr.log").open("w")
        self.process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=self.errors)
        deadline = time.monotonic() + 10
        while not self.qmp_path.exists():
            if self.process.poll() is not None or time.monotonic() > deadline: raise RuntimeError("QEMU launch failed")
            time.sleep(0.05)
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.settimeout(20)
        self.socket.connect(str(self.qmp_path))
        self.wire = self.socket.makefile("rwb")
        self.wire.readline()
        self.qmp("qmp_capabilities")
        self.shots = 0

    def qmp(self, name, arguments=None):
        request = {"execute": name}
        if arguments is not None: request["arguments"] = arguments
        self.wire.write(json.dumps(request).encode() + b"\n")
        self.wire.flush()
        while True:
            response = json.loads(self.wire.readline())
            if "event" in response: continue
            if "error" in response: raise RuntimeError(str(response))
            return response["return"]

    def serial(self):
        file = self.output / "serial.log"
        return file.read_text(errors="replace").replace("\r", "") if file.exists() else ""

    def wait(self, expression, offset=0, seconds=60):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            data = self.serial()[offset:]
            if re.search(r"KERNEL_EXCEPTION|KERNEL_PANIC|ALLOCATION ERROR|USER_PANIC|USER_OOM", data):
                raise RuntimeError("kernel or user fault: " + data[-2000:])
            match = re.search(expression, data)
            if match: return match
            if self.process.poll() is not None: raise RuntimeError("QEMU exited")
            time.sleep(0.1)
        raise RuntimeError(f"missing {expression!r}; serial tail: {self.serial()[-1500:]}")

    def shot(self, name):
        self.shots += 1
        ppm = self.output / f"{self.shots:02d}-{name}.ppm"
        self.qmp("screendump", {"filename": str(ppm)})
        time.sleep(0.2)
        width, height, rgb = read_ppm(ppm)
        write_png(ppm.with_suffix(".png"), width, height, rgb)
        ppm.unlink()
        return Shot(width, height, rgb)

    def shot_until(self, name, check, seconds=30):
        """Screenshots until `check(shot)` holds: frames are rendered in software, so
        the screen can lag the input that is already processed."""
        deadline = time.monotonic() + seconds
        while True:
            s = self.shot(name)
            if check(s) or time.monotonic() > deadline:
                return s
            time.sleep(1)

    def move(self, dx, dy):
        while dx or dy:
            sx, sy = max(-100, min(100, dx)), max(-100, min(100, dy))
            self.qmp("input-send-event", {"events": [{"type": "rel", "data": {"axis": "x", "value": sx}},
                                                     {"type": "rel", "data": {"axis": "y", "value": sy}}]})
            dx, dy = dx - sx, dy - sy
            time.sleep(0.02)

    def goto(self, x, y):
        self.move(-SCREEN[0] - 100, -SCREEN[1] - 100)  # The pointer clamps at the top-left corner.
        time.sleep(0.15)
        self.move(x, y)
        time.sleep(0.25)

    def button(self, down):
        self.qmp("input-send-event", {"events": [{"type": "btn", "data": {"button": "left", "down": down}}]})

    def click(self, x, y, double=False):
        self.goto(x, y)
        for _ in range(2 if double else 1):
            self.button(True); time.sleep(0.04); self.button(False); time.sleep(0.06)
        time.sleep(0.6)

    def drag(self, x0, y0, x1, y1):
        self.goto(x0, y0)
        self.button(True); time.sleep(0.2)
        self.move(x1 - x0, y1 - y0); time.sleep(0.4)
        self.button(False); time.sleep(0.6)

    def keys(self, text):
        names = {" ": "spc", "\n": "ret", ".": "dot", "-": "minus", "/": "slash", ",": "comma", "=": "equal",
                 "_": ("shift", "minus"), ">": ("shift", "dot"), "|": ("shift", "backslash"), "!": ("shift", "1"),
                 "@": ("shift", "2"), "#": ("shift", "3"), ":": ("shift", "semicolon"), "+": ("shift", "equal"),
                 "*": ("shift", "8")}
        for ch in text:
            key = names.get(ch, ch)
            keys = list(key) if isinstance(key, tuple) else (["shift", ch.lower()] if ch.isupper() else [key])
            self.qmp("send-key", {"keys": [{"type": "qcode", "data": k} for k in keys], "hold-time": 30})
            time.sleep(0.05)

    def combo(self, *keys):
        self.qmp("send-key", {"keys": [{"type": "qcode", "data": k} for k in keys], "hold-time": 50})
        time.sleep(0.4)

    def start(self):
        self.click(32, SCREEN[1] - START_FROM_BOTTOM)

    def menu(self, item):
        self.start()
        self.click(110, SCREEN[1] - MENU_FROM_BOTTOM[item])

    def close(self):
        if self.process.poll() is None:
            try: self.qmp("quit")
            except Exception: pass
            try: self.process.wait(timeout=10)
            except subprocess.TimeoutExpired: self.process.kill(); self.process.wait()
        self.wire.close(); self.socket.close(); self.errors.close()
        self.qmp_path.unlink(missing_ok=True)
        self.control.cleanup()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--accel", choices=["kvm", "tcg"], default="tcg")
    parser.add_argument("--memory", default="8G")
    parser.add_argument("--resolution", default="1920x1080", help="the virtual monitor's preferred mode")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    disk = output / "data.img"
    with disk.open("xb") as stream: stream.truncate(8 * 1024 * 1024)
    resolution = tuple(int(v) for v in args.resolution.split("x"))
    result = {"acceleration": args.accel, "memory": args.memory, "resolution": args.resolution, "checks": {}}
    started = time.monotonic()
    vm = None
    note = "Notes typed in the Atom desktop: ABC xyz !@# > |"

    def passed(name):
        result["checks"][name] = True
        print(name + " PASS", flush=True)

    try:
        global SCREEN, SCALE
        vm = Desktop(args.source, output / "session", disk, args.accel, args.memory, resolution)
        match = vm.wait(r"DESKTOP_READY (\d+)x(\d+) scale=(\d+)", seconds=240)
        assert (int(match[1]), int(match[2])) == resolution, match[0]
        SCALE = int(match[3])
        SCREEN = (resolution[0] // SCALE, resolution[1] // SCALE)
        result["scale"] = SCALE
        width, height = SCREEN
        time.sleep(2)
        s = vm.shot_until("desktop", lambda s: max(s.pixel(600, height - 8)) < 70, seconds=120)
        assert (s.width, s.height) == resolution, (s.width, s.height)
        assert max(s.pixel(600, height - 8)) < 70, s.pixel(600, height - 8)  # Taskbar.
        assert s.pixel(600, 300)[2] > s.pixel(600, 300)[0], s.pixel(600, 300)  # Blue wallpaper.
        passed(f"DESKTOP_BOOT_{resolution[0]}x{resolution[1]}_SCALE_{SCALE}")

        vm.start()
        menu_probe = (150, height - 348)
        s = vm.shot_until("start-menu", lambda s: s.near(*menu_probe, (28, 32, 46), 6))
        assert s.near(*menu_probe, (28, 32, 46), 6), s.pixel(*menu_probe)
        vm.combo("esc")
        passed("START_MENU")

        # Windowed programs: separate processes drawn by the desktop from the intent
        # tree they send. With no other window open, the Calculator opens at the first
        # window position (140, 40); the "5" key sits near (254, 315) in its layout.
        offset = len(vm.serial())
        vm.menu("calculator")
        vm.wait(r"WINDOW_OPEN Calculator", offset)
        vm.wait(r"CALC_DISPLAY 0\n", offset)
        time.sleep(1)
        offset = len(vm.serial())
        vm.keys("12+30\n")  # Declared keyboard shortcuts press the buttons.
        vm.wait(r"CALC_DISPLAY 42\n", offset)
        offset = len(vm.serial())
        vm.click(254, 315)  # The mouse: the desktop's layout and hit test find the button.
        vm.wait(r"CALC_DISPLAY 5\n", offset)
        time.sleep(1)
        vm.shot("calculator")
        offset = len(vm.serial())
        vm.combo("esc")
        vm.wait(r"CALC_DISPLAY 0\n", offset)
        vm.combo("alt", "f4")
        time.sleep(1)
        offset = len(vm.serial())
        vm.menu("about")
        vm.wait(r"WINDOW_OPEN About Atom OS", offset)
        time.sleep(2)
        vm.shot("about")
        vm.combo("alt", "f4")
        time.sleep(1)
        passed("WINDOWED_PROGRAMS")

        # Notepad: a windowed program with a text area the desktop edits. It opens at
        # the first window position too; its text area covers (460, 300).
        offset = len(vm.serial())
        vm.menu("notepad")
        vm.wait(r"WINDOW_OPEN Notepad", offset)
        time.sleep(1.5)
        vm.click(460, 300)
        typed = "Hello from Notepad\nSecond line"
        vm.keys(typed)
        vm.combo("ctrl", "s")  # A declared Ctrl shortcut, even while the text area types.
        match = vm.wait(r"NOTEPAD_SAVED /notepad\.txt (\d+)", offset)
        assert int(match[1]) == len(typed), match[0]
        offset = len(vm.serial())
        vm.combo("ctrl", "n")
        vm.combo("ctrl", "o")
        match = vm.wait(r"NOTEPAD_OPENED /notepad\.txt (\d+)", offset)
        assert int(match[1]) == len(typed), match[0]
        time.sleep(1)
        vm.shot("notepad")
        vm.combo("alt", "f4")
        time.sleep(1)
        passed("NOTEPAD")

        # Editor: Save on an untitled document opens the Save As picker.
        vm.menu("editor")
        vm.wait(r"WINDOW_OPEN Untitled - Text Editor")
        time.sleep(1)
        vm.keys(note)
        vm.combo("ctrl", "s")
        vm.wait(r"WINDOW_OPEN Save As")
        time.sleep(1)
        vm.shot("save-as-picker")
        vm.keys("notes.txt\n")
        vm.wait(r"TOAST Saved notes\.txt")
        vm.combo("ctrl", "shift", "s")
        vm.wait(r"WINDOW_OPEN Save As[\s\S]*WINDOW_OPEN Save As")
        vm.keys("copy.txt\n")
        vm.wait(r"TOAST Saved copy\.txt")
        passed("EDITOR_SAVE_AS_PICKER")

        vm.combo("ctrl", "o")
        vm.wait(r"WINDOW_OPEN Open File")
        time.sleep(1)
        vm.shot("open-picker")
        vm.keys("notes.txt\n")
        match = vm.wait(r"EDITOR_OPEN /notes\.txt (\d+) bytes")
        assert int(match[1]) == len(note), match[0]
        vm.shot("editor")
        passed("FILE_PICKER_OPEN")

        # Terminal: the shell runs over pipes and its redirection creates a file.
        vm.menu("terminal")
        vm.wait(r"WINDOW_OPEN Terminal")
        time.sleep(6)
        vm.keys("echo hello-from-terminal > term.txt\n")
        time.sleep(2)
        vm.keys("mkdir docs\n")
        time.sleep(2)
        vm.keys("spawn sleeper.elf\n")
        time.sleep(3)
        vm.shot("terminal")
        vm.menu("editor")
        time.sleep(1)
        vm.combo("ctrl", "o")
        vm.wait(r"WINDOW_OPEN Open File[\s\S]*WINDOW_OPEN Open File")
        vm.keys("term.txt\n")
        match = vm.wait(r"EDITOR_OPEN /term\.txt (\d+) bytes")
        assert int(match[1]) == len("hello-from-terminal\n"), match[0]
        passed("TERMINAL_SHELL_OVER_PIPES")

        # Folders in the picker: save into the folder the terminal made, then browse
        # back up and into it by typing folder names, and open the file from there.
        offset = len(vm.serial())
        vm.combo("ctrl", "shift", "s")
        vm.wait(r"WINDOW_OPEN Save As", offset)
        time.sleep(1)
        vm.keys("docs/report.txt\n")
        vm.wait(r"TOAST Saved report\.txt", offset)
        vm.combo("ctrl", "o")
        vm.wait(r"WINDOW_OPEN Open File", offset)
        time.sleep(1)
        vm.shot("picker-in-folder")
        vm.keys("..\n")
        vm.wait(r"PICKER_DIR /\n", offset)
        vm.keys("docs\n")
        vm.wait(r"PICKER_DIR /docs\n", offset)
        vm.keys("report.txt\n")
        match = vm.wait(r"EDITOR_OPEN /docs/report\.txt (\d+) bytes", offset)
        assert int(match[1]) == len("hello-from-terminal\n"), match[0]
        passed("PICKER_FOLDERS")

        # System monitor: the newest process is the sleeper; end it.
        vm.menu("monitor")
        vm.wait(r"WINDOW_OPEN System Monitor")
        time.sleep(1.5)
        vm.combo("end")
        vm.combo("delete")
        vm.wait(r"WINDOW_OPEN End Process")
        time.sleep(1)
        vm.shot("end-process")
        vm.combo("ret")
        vm.wait(r"TOAST Ended process \d+")
        passed("MONITOR_END_PROCESS")

        # Window management on the monitor (fourth top-level window, cascade slot 3).
        x, y = 140 + 3 * 32, 40 + 3 * 28
        vm.click(x + 150, y + 16, double=True)
        vm.wait(r"WINDOW_MAXIMIZE System Monitor")
        s = vm.shot_until("maximized", lambda s: s.near(300, 12, TITLE_ACTIVE, 4))
        assert s.near(300, 12, TITLE_ACTIVE, 4), s.pixel(300, 12)
        vm.click(300, 16, double=True)
        vm.wait(r"WINDOW_RESTORE System Monitor")
        time.sleep(2)
        vm.drag(x + 150, y + 16, x + 350, y + 216)
        s = vm.shot_until("dragged", lambda s: s.near(x + 450, y + 216, TITLE_ACTIVE, 4))
        assert s.near(x + 450, y + 216, TITLE_ACTIVE, 4), s.pixel(x + 450, y + 216)
        passed("WINDOW_MAXIMIZE_RESTORE_DRAG")

        vm.menu("files")
        vm.wait(r"WINDOW_OPEN Files")
        time.sleep(1.5)
        vm.shot("files")
        vm.menu("sync")
        vm.wait(r"TOAST All files saved to disk")
        passed("SAVE_ALL_TO_DISK")

        offset = len(vm.serial())
        vm.menu("exit")
        vm.wait(r"DESKTOP_EXIT", offset)
        vm.wait(r"Desktop closed \(status 0\)", offset)
        time.sleep(1)
        vm.keys("cat notes.txt\n")
        vm.wait(re.escape(note), offset)
        s = vm.shot("text-console")
        # The console is the 320x200 graphical console (mode 13h), which QEMU
        # captures at 640x400; its blue taskbar fills the bottom row.
        assert (s.width, s.height) == (640, 400), (s.width, s.height)
        r, g, b = s.raw(10, 395)
        assert b > 120 and r < 60 and g < 60, ("taskbar", (r, g, b))
        vm.keys("desktop\n")
        vm.wait(r"DESKTOP_READY", offset)
        time.sleep(2)
        s = vm.shot("desktop-again")
        assert (s.width, s.height) == resolution, (s.width, s.height)
        passed("EXIT_TO_TEXT_CONSOLE_AND_BACK")

        offset = len(vm.serial())
        vm.menu("restart")
        vm.wait(r"STORAGE_READY generation=\d+", offset, seconds=120)
        vm.wait(r"DESKTOP_READY", offset, seconds=240)
        time.sleep(2)
        vm.menu("editor")
        time.sleep(1)
        vm.combo("ctrl", "o")
        vm.wait(r"WINDOW_OPEN Open File", offset)
        vm.keys("notes.txt\n")
        match = vm.wait(r"EDITOR_OPEN /notes\.txt (\d+) bytes", offset)
        assert int(match[1]) == len(note), match[0]
        vm.shot("after-restart")
        passed("RESTART_PERSISTENCE")
        result["success"] = True
    except Exception as error:
        result["success"] = False
        result["error"] = str(error)
        print("FAIL: " + str(error), flush=True)
        if vm and vm.process.poll() is None:
            try: vm.shot("failure")
            except Exception: pass
    finally:
        if vm: vm.close()
        result["elapsed_seconds"] = round(time.monotonic() - started, 3)
        (output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return 0 if result["success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
