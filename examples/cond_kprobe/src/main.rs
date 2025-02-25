// SPDX-License-Identifier: (LGPL-2.1 OR BSD-2-Clause)

#![allow(clippy::let_unit_value)]

use std::ffi::c_int;
use std::ffi::c_void;
use std::ffi::OsStr;
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::mem::MaybeUninit;
use std::net::TcpListener;
use std::net::TcpStream;
use std::os::fd::AsFd as _;
use std::os::fd::AsRawFd as _;
use std::os::fd::BorrowedFd;
use std::os::unix::ffi::OsStrExt as _;
use std::ptr::copy_nonoverlapping;
use std::thread;

use clap::Parser;

use libbpf_rs::skel::OpenSkel;
use libbpf_rs::skel::SkelBuilder;
use libbpf_rs::AsRawLibbpf as _;
use libbpf_rs::ErrorExt as _;
use libbpf_rs::ErrorKind;
use libbpf_rs::Result;

use crate::cond_kprobe::CondKprobeSkelBuilder;

mod cond_kprobe {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bpf/cond_kprobe.skel.rs"
    ));
}


/// An example program adding a TCP congestion algorithm.
#[derive(Debug, Parser)]
struct Args {
    /// Verbose debug output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    Ok(())
}
