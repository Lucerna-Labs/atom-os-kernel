#!/usr/bin/env python3
"""Exercise the real preflight executable with a built ELF and hostile headers."""
import argparse
import json
from pathlib import Path
import struct
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path, required=True)
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    original = args.elf.read_bytes()
    phoff = struct.unpack_from("<Q", original, 32)[0]
    phnum = struct.unpack_from("<H", original, 56)[0]
    load = next(phoff + 56 * i for i in range(phnum)
                if struct.unpack_from("<I", original, phoff + 56 * i)[0] == 1
                and struct.unpack_from("<Q", original, phoff + 56 * i + 40)[0])
    cases = {"valid": original, "truncated": original[:63]}
    for name, offset, fmt, value in [
        ("wrong_machine", 18, "<H", 183),
        ("overflowing_headers", 32, "<Q", (1 << 64) - 1),
        ("writable_executable", load + 4, "<I", 7),
        ("entry_outside_image", 24, "<Q", 0),
    ]:
        data = bytearray(original)
        struct.pack_into(fmt, data, offset, value)
        cases[name] = data
    cases["bad_magic"] = b"FAIL" + original[4:]
    report = {}
    for name, data in cases.items():
        path = args.output / (name + ".elf")
        path.write_bytes(data)
        proc = subprocess.run([str(args.checker), str(path)], capture_output=True, text=True)
        expected = name == "valid"
        assert (proc.returncode == 0) == expected, (name, proc.stdout, proc.stderr)
        report[name] = {"accepted": proc.returncode == 0, "output": proc.stdout + proc.stderr}
    for name, paths in [("missing", [args.output / "missing.elf"]),
                        ("mixed_batch", [args.elf, args.output / "bad_magic.elf"]),
                        ("no_arguments", [])]:
        proc = subprocess.run([str(args.checker), *map(str, paths)], capture_output=True, text=True)
        assert proc.returncode != 0, name
        report[name] = {"accepted": False, "output": proc.stdout + proc.stderr}
    (args.output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"ELF_PREFLIGHT_TESTS_OK cases={len(report)}")


if __name__ == "__main__":
    main()
