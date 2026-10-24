# Supplemental regression tests

These tests supplement the course checker; they do not replace it or assert an official score.

- Chapter 3: `rustc --test tests/trace.rs -o /tmp/trace-tests && /tmp/trace-tests` on `ch3`.
- Chapters 4/5/6/8: `cargo test --manifest-path tests/vm/Cargo.toml` directly exercises the actual page-table/memory-set implementation with host-backed physical frames. From chapter 5 it also tests the actual scheduler's dispatch ratios.
- Chapters 6/8: `cargo test --manifest-path easy-fs/Cargo.toml` covers hard links, open/unlink lifetime, reclamation, directory slot reuse and persistence using a memory block device.
- Chapter 8: `rustc --edition=2021 --test os/src/sync/deadlock.rs -o /tmp/deadlock-tests && /tmp/deadlock-tests` checks the actual detector independently of context-switch assembly.
- QEMU: `python3 tests/smoke/run.py ch8` (substitute the currently checked-out chapter) builds a disposable kernel copy under `tmp/` plus dependency-free user programs, then checks syscall results, process lifecycle, file operations and mutex/semaphore cycles. Each invocation needs a fresh disposable directory. `--kernel-dir PATH` uses a prepared disposable chapter root and may overwrite its test guest files.

Use the repository toolchain. QEMU and rust-objcopy must be on PATH. The default smoke runner uses the supplied RustSBI. `--bios default` is available for supplemental testing on older QEMU with its bundled OpenSBI; this is a different environment from the course's QEMU 8.2.2 and RustSBI 0.4.1. `--offline --vendor-dir PATH` can reuse existing crate caches without changing repository dependency declarations.

Reports distinguish host checks and any compatibility-adapted local smoke builds from official validation. The official requirement remains `make -C ci-user test CHAPTER=N`, including its report check and successful exit status.
