use std::env;
use std::ffi::OsStr;
use std::fmt::Arguments;
use std::path::PathBuf;

use log::set_logger;
use log::set_max_level;
use log::LevelFilter;
use log::Log;
use log::Metadata;
use log::Record;

use libbpf_cargo::SkeletonBuilder;

const SRC: &str = "src/bpf/compiler_warnings.bpf.c";


fn for_each_line<F>(args: &Arguments<'_>, f: F)
where
    F: FnMut(&str),
{
    fn split_lines(b: char) -> bool {
        b == '\n'
    }

    if let Some(s) = args.as_str() {
        s.split(split_lines).for_each(f)
    } else {
        format!("{args}").split(split_lines).for_each(f)
    }
}

struct CompilerWarningsLogger;

impl Log for CompilerWarningsLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if record.metadata().target() == "compiler-stderr" {
            for_each_line(record.args(), |line| println!("cargo:warning={}", line));
        }
    }

    fn flush(&self) {}
}

fn main() {
    let out = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build script"),
    )
    .join("src")
    .join("bpf")
    .join("compiler_warnings.skel.rs");

    let arch = env::var("CARGO_CFG_TARGET_ARCH")
        .expect("CARGO_CFG_TARGET_ARCH must be set in build script");

    set_logger(&CompilerWarningsLogger).unwrap();
    set_max_level(LevelFilter::Info);

    SkeletonBuilder::new()
        .source(SRC)
        .clang_args([
            OsStr::new("-I"),
            vmlinux::include_path_root().join(arch).as_os_str(),
        ])
        .build_and_generate(&out)
        .unwrap();

    println!("cargo:rerun-if-changed={SRC}");
}
