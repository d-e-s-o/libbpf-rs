// SPDX-License-Identifier: (LGPL-2.1 OR BSD-2-Clause)

use core::time::Duration;
use std::ffi::c_int;
use std::mem::MaybeUninit;
use std::os::fd::AsFd as _;

use plain::Plain;

use libbpf_rs::skel::OpenSkel;
use libbpf_rs::skel::Skel;
use libbpf_rs::skel::SkelBuilder;
use libbpf_rs::MapHandle;
use libbpf_rs::MapType;
use libbpf_rs::Result;

mod ringbuf_multi {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/bpf/ringbuf_multi.skel.rs"
    ));
}

use ringbuf_multi::*;

unsafe impl Plain for types::sample {}

fn process_sample(ring: c_int, data: &[u8]) -> i32 {
    let s = plain::from_bytes::<types::sample>(data).unwrap();

    match s.seq {
        0 => {
            assert_eq!(ring, 1);
            assert_eq!(s.value, 333);
            0
        }
        1 => {
            assert_eq!(ring, 2);
            assert_eq!(s.value, 777);
            0
        }
        _ => unreachable!(),
    }
}

fn main() -> Result<()> {
    let skel_builder = RingbufMultiSkelBuilder::default();
    let mut open_object = MaybeUninit::uninit();
    let mut open_skel = skel_builder.open(&mut open_object)?;

    let entries = open_skel.maps.ringbuf1.max_entries();
    let mut opts = libbpf_rs::libbpf_sys::bpf_map_create_opts::default();
    opts.sz = size_of_val(&opts) as _;
    let map =
        MapHandle::create(MapType::RingBuf, Some("ringbuf_hash"), 0, 0, entries, &opts).unwrap();
    let () = open_skel
        .maps
        .ringbuf_hash
        .set_inner_map_fd(map.as_fd())
        .unwrap();

    let mut skel = open_skel.load()?;
    drop(map);

    // Only trigger BPF program for current process.
    let pid = unsafe { libc::getpid() };
    skel.maps.bss_data.pid = pid;

    let mut builder = libbpf_rs::RingBufferBuilder::new();
    builder
        .add(&skel.maps.ringbuf1, |data| process_sample(1, data))
        .expect("failed to add ringbuf");
    builder
        .add(&skel.maps.ringbuf2, |data| process_sample(2, data))
        .expect("failed to add ringbuf");
    let ringbuf = builder.build().unwrap();

    let () = skel.attach()?;

    // trigger few samples, some will be skipped
    skel.maps.bss_data.target_ring = 0;
    skel.maps.bss_data.value = 333;
    let _pgid = unsafe { libc::getpgid(pid) };

    // skipped, no ringbuf in slot 1
    skel.maps.bss_data.target_ring = 1;
    skel.maps.bss_data.value = 555;
    let _pgid = unsafe { libc::getpgid(pid) };

    skel.maps.bss_data.target_ring = 2;
    skel.maps.bss_data.value = 777;
    let _pgid = unsafe { libc::getpgid(pid) };

    // poll for samples, should get 2 ringbufs back
    let n = ringbuf.poll_raw(Duration::MAX);
    assert_eq!(n, 2);
    println!("successfully polled {n} samples");

    // expect extra polling to return nothing
    let n = ringbuf.poll_raw(Duration::from_secs(0));
    assert!(n == 0, "{n}");
    Ok(())
}
