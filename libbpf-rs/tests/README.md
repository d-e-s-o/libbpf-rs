# libbpf-rs tests

libbpf-rs tests are designed to be independent of libbpf-cargo. To that end they
work with `libbpf_rs::Object` directly and load pre-compiled BPF object files
from `libbpf-rs/tests/bin`.

These object files are not checked in. Every `*.bpf.c` file in
`libbpf-rs/tests/bin/src` is compiled into a corresponding `*.bpf.o` by the
`libbpf-rs-dev` build script (`libbpf-rs/dev/build.rs`), which runs as part of
building the tests and requires nothing but `clang` in `PATH`. Adding a test
program is a matter of dropping a new `*.bpf.c` into that directory.

The one exception to the above is functionality that is only reachable through a
generated skeleton, such as BPF arena global variables. For those, the same
build script additionally invokes `libbpf_cargo::SkeletonBuilder` to emit a
`*.skel.rs` next to the object file; see the `SKELETONS` list in
`libbpf-rs/dev/build.rs`. Please keep this list short and prefer working with
`Object` where that is possible at all.

Note that `bin/src/arena.bpf.c` requires clang 19 or newer, which is the first
release defining `__BPF_FEATURE_ADDR_SPACE_CAST`.
