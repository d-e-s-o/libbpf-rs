//! # libbpf-rs
//!
//! **libbpf-rs** is a safe, idiomatic, and opinionated wrapper around
//! [`libbpf`](https://github.com/libbpf/libbpf/). It provides
//! primitives and building blocks for loading and interacting with BPF
//! kernel programs.
//!
//! **libbpf-rs** comes with a companion library and Cargo build system
//! plugin, [**libbpf-cargo**][libbpf-cargo], which can be used for
//! compiling BPF C code and generating a Rust "skeleton" based on it.
//! In most scenarios and when you are starting from scratch,
//! **libbpf-cargo** is where you should begin your journey.
//!
//! Additionally, please refer to the various [examples][] combining
//! both libraries and illustrating various workflows.
//!
//! [libbpf-cargo]: https://docs.rs/libbpf-cargo
//! [examples]: https://github.com/libbpf/libbpf-rs/tree/master/examples

#![allow(clippy::let_and_return, clippy::let_unit_value)]
#![warn(
    elided_lifetimes_in_paths,
    missing_debug_implementations,
    missing_docs,
    single_use_lifetimes,
    clippy::absolute_paths,
    clippy::wildcard_imports,
    rustdoc::broken_intra_doc_links
)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod btf;
mod error;
mod iter;
mod link;
mod linker;
mod map;
mod netfilter;
mod object;
mod perf_buffer;
mod print;
mod program;
pub mod query;
mod ringbuf;
mod skeleton;
mod tc;
mod user_ringbuf;
mod util;
mod xdp;

pub use libbpf_sys;

pub use crate::btf::Btf;
pub use crate::btf::HasSize;
pub use crate::btf::ReferencesType;
pub use crate::error::Error;
pub use crate::error::ErrorExt;
pub use crate::error::ErrorKind;
pub use crate::error::Result;
pub use crate::iter::Iter;
pub use crate::link::Link;
pub use crate::linker::Linker;
pub use crate::map::Map;
pub use crate::map::MapCore;
pub use crate::map::MapFlags;
pub use crate::map::MapHandle;
pub use crate::map::MapImpl;
pub use crate::map::MapInfo;
pub use crate::map::MapKeyIter;
pub use crate::map::MapMut;
pub use crate::map::MapType;
pub use crate::map::OpenMap;
pub use crate::map::OpenMapImpl;
pub use crate::map::OpenMapMut;
pub use crate::netfilter::NetfilterOpts;
pub use crate::netfilter::NFPROTO_IPV4;
pub use crate::netfilter::NFPROTO_IPV6;
pub use crate::netfilter::NF_INET_FORWARD;
pub use crate::netfilter::NF_INET_LOCAL_IN;
pub use crate::netfilter::NF_INET_LOCAL_OUT;
pub use crate::netfilter::NF_INET_POST_ROUTING;
pub use crate::netfilter::NF_INET_PRE_ROUTING;
pub use crate::object::AsRawLibbpf;
pub use crate::object::MapIter;
pub use crate::object::Object;
pub use crate::object::ObjectBuilder;
pub use crate::object::OpenObject;
pub use crate::object::ProgIter;
pub use crate::perf_buffer::PerfBuffer;
pub use crate::perf_buffer::PerfBufferBuilder;
pub use crate::print::get_print;
pub use crate::print::set_print;
pub use crate::print::PrintCallback;
pub use crate::print::PrintLevel;
pub use crate::program::Input as ProgramInput;
pub use crate::program::OpenProgram;
pub use crate::program::OpenProgramImpl;
pub use crate::program::OpenProgramMut;
pub use crate::program::Output as ProgramOutput;
pub use crate::program::Program;
pub use crate::program::ProgramAttachType;
pub use crate::program::ProgramImpl;
pub use crate::program::ProgramMut;
pub use crate::program::ProgramType;
pub use crate::program::TracepointOpts;
pub use crate::program::UprobeOpts;
pub use crate::program::UsdtOpts;
pub use crate::ringbuf::RingBuffer;
pub use crate::ringbuf::RingBufferBuilder;
pub use crate::tc::TcAttachPoint;
pub use crate::tc::TcHook;
pub use crate::tc::TcHookBuilder;
pub use crate::tc::TC_CUSTOM;
pub use crate::tc::TC_EGRESS;
pub use crate::tc::TC_H_CLSACT;
pub use crate::tc::TC_H_INGRESS;
pub use crate::tc::TC_H_MIN_EGRESS;
pub use crate::tc::TC_H_MIN_INGRESS;
pub use crate::tc::TC_INGRESS;
pub use crate::user_ringbuf::UserRingBuffer;
pub use crate::user_ringbuf::UserRingBufferSample;
pub use crate::util::num_possible_cpus;
pub use crate::xdp::Xdp;
pub use crate::xdp::XdpFlags;

/// An unconstructible dummy type used for tagging mutable type
/// variants.
#[doc(hidden)]
#[derive(Copy, Clone, Debug)]
pub enum Mut {}


/// Used for skeleton -- an end user may not consider this API stable
#[doc(hidden)]
pub mod __internal_skel {
    pub use super::skeleton::*;
}

/// Skeleton related definitions.
pub mod skel {
    pub use super::skeleton::OpenSkel;
    pub use super::skeleton::Skel;
    pub use super::skeleton::SkelBuilder;
}
