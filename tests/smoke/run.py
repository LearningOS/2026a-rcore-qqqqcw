#!/usr/bin/env python3
"""Build a standalone smoke guest and run it against a disposable kernel copy."""
import argparse
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("chapter", choices=["ch3", "ch4", "ch5", "ch6", "ch8"])
parser.add_argument("--kernel-dir", type=Path, help="Existing disposable chapter root (including os/)")
parser.add_argument("--bios", default=str(ROOT / "bootloader/rustsbi-qemu.bin"))
parser.add_argument("--offline", action="store_true")
parser.add_argument("--vendor-dir", type=Path, help="Optional cached crates.io directory")
parser.add_argument("--mode", choices=["release", "debug"], default="release")
parser.add_argument("--tasks", type=int, default=1,
                    help="Number of independent chapter 3 guests (1 to 16)")
args = parser.parse_args()
chapter = args.chapter
if not 1 <= args.tasks <= 16 or (chapter != "ch3" and args.tasks != 1):
    parser.error("--tasks must be 1 to 16 for ch3, and 1 for other chapters")
if args.kernel_dir:
    dest = args.kernel_dir.resolve()
else:
    dest = ROOT / "tmp/smoke" / chapter
    if dest.exists():
        raise SystemExit(f"{dest} already exists; select a fresh --kernel-dir or remove this disposable copy")
    for name in ["os", "easy-fs"]:
        if (ROOT / name).exists():
            shutil.copytree(ROOT / name, dest / name,
                            ignore=shutil.ignore_patterns("target", "Cargo.lock", "link_app.S"))

out = dest / "user/build/elf"
out.mkdir(parents=True, exist_ok=True)
for old in out.glob("*.elf"):
    old.unlink()
link = dest / "user/linker.ld"
start = 0x80400000 if chapter == "ch3" else 0x10000
name = chapter + "b_initproc"
guests = [(index, False) for index in range(args.tasks)]
if chapter in ("ch5", "ch6", "ch8"):
    guests.append((0, True))
if chapter == "ch3":
    binpath = dest / "user/build/bin"
    binpath.mkdir(parents=True, exist_ok=True)
    for old in binpath.glob("*.bin"):
        old.unlink()
for index, child in guests:
    address = start + index * 0x20000
    link.write_text("OUTPUT_ARCH(riscv)\nENTRY(_start)\nSECTIONS { . = " + hex(address) +
                    "; .text : { *(.text.entry) *(.text .text.*) } . = ALIGN(4096); "
                    ".rodata : { *(.rodata .rodata.*) } . = ALIGN(4096); "
                    ".data : { *(.data .data.*) *(.sdata .sdata.*) } "
                    ".bss : { *(.bss .bss.*) *(.sbss .sbss.*) } /DISCARD/ : { *(.eh_frame) } }")
    guest_name = "child" if child else name
    if chapter == "ch3":
        guest_name += f"{index:02}"
    target = out / (guest_name + ".elf")
    cmd = ["rustc", "+nightly-2024-05-02", "--edition=2021", "--target",
           "riscv64gc-unknown-none-elf", "-C", "opt-level=2", "-C", "panic=abort",
           "-C", "link-arg=-T" + str(link), "--cfg", chapter,
           str(ROOT / "tests/smoke/guest.rs"), "-o", str(target)]
    if child:
        cmd += ["--cfg", "child"]
    subprocess.run(cmd, check=True)
    if chapter == "ch3":
        subprocess.run(["rust-objcopy", "--strip-all", "-O", "binary",
                        str(target), str(binpath / (guest_name + ".bin"))], check=True)

cargo = ["--offline"] if args.offline else []
if args.vendor_dir:
    cargo += ["--config", 'source.crates-io.replace-with="local-vendor"', "--config",
              'source.local-vendor.directory="' + str(args.vendor_dir.resolve()) + '"']
mode_flags = ["--release"] if args.mode == "release" else []
subprocess.run(["cargo", "+nightly-2024-05-02", "build"] + mode_flags + [
                "--target", "riscv64gc-unknown-none-elf"] + cargo, cwd=dest / "os", check=True)
kernel = dest / "os/target/riscv64gc-unknown-none-elf" / args.mode / "os"
cmd = ["qemu-system-riscv64", "-machine", "virt", "-nographic", "-bios", args.bios,
       "-kernel", str(kernel)]
if chapter in ("ch6", "ch8"):
    image = dest / "smoke.img"
    subprocess.run(["cargo", "+nightly-2024-05-02", "run", "--manifest-path",
                    str(ROOT / "tests/smoke/fs-image/Cargo.toml")] + cargo + ["--", str(image),
                    name, str(out / (name + ".elf")), "child", str(out / "child.elf")], check=True)
    cmd += ["-drive", "file=" + str(image) + ",if=none,format=raw,id=x0", "-device",
            "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0"]
try:
    result = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
    log = result.stdout.decode(errors="replace")
except subprocess.TimeoutExpired as error:
    log = (error.stdout or b"").decode(errors="replace")
    result = None
(dest / "smoke.log").write_text(log)
print(log)
if result is None or result.returncode != 0 or log.count("SMOKE PASS") != args.tasks \
        or "SMOKE FAIL" in log or "SMOKE PANIC" in log:
    raise SystemExit(1)
