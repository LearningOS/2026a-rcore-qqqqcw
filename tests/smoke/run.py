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
args = parser.parse_args()
chapter = args.chapter
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
start = "0x80400000" if chapter == "ch3" else "0x10000"
link.write_text("OUTPUT_ARCH(riscv)\nENTRY(_start)\nSECTIONS { . = " + start +
                "; .text : { *(.text.entry) *(.text .text.*) } . = ALIGN(4096); "
                ".rodata : { *(.rodata .rodata.*) } . = ALIGN(4096); "
                ".data : { *(.data .data.*) *(.sdata .sdata.*) } "
                ".bss : { *(.bss .bss.*) *(.sbss .sbss.*) } /DISCARD/ : { *(.eh_frame) } }")
name = chapter + "b_initproc"
for child in [False, True] if chapter in ("ch5", "ch6", "ch8") else [False]:
    target = out / ("child.elf" if child else name + ".elf")
    cmd = ["rustc", "+nightly-2024-05-02", "--edition=2021", "--target",
           "riscv64gc-unknown-none-elf", "-C", "opt-level=2", "-C", "panic=abort",
           "-C", "link-arg=-T" + str(link), "--cfg", chapter,
           str(ROOT / "tests/smoke/guest.rs"), "-o", str(target)]
    if child:
        cmd += ["--cfg", "child"]
    subprocess.run(cmd, check=True)
if chapter == "ch3":
    binpath = dest / "user/build/bin"
    binpath.mkdir(parents=True, exist_ok=True)
    for old in binpath.glob("*.bin"):
        old.unlink()
    subprocess.run(["rust-objcopy", "--strip-all", "-O", "binary",
                    str(out / (name + ".elf")), str(binpath / (name + ".bin"))], check=True)

cargo = ["--offline"] if args.offline else []
if args.vendor_dir:
    cargo += ["--config", 'source.crates-io.replace-with="local-vendor"', "--config",
              'source.local-vendor.directory="' + str(args.vendor_dir.resolve()) + '"']
subprocess.run(["cargo", "+nightly-2024-05-02", "build", "--release",
                "--target", "riscv64gc-unknown-none-elf"] + cargo, cwd=dest / "os", check=True)
kernel = dest / "os/target/riscv64gc-unknown-none-elf/release/os"
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
if result is None or result.returncode != 0 or "SMOKE PASS" not in log \
        or "SMOKE FAIL" in log or "SMOKE PANIC" in log:
    raise SystemExit(1)
