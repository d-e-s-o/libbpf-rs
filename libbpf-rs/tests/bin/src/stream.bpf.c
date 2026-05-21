// SPDX-License-Identifier: GPL-2.0
/* Copyright (c) 2025 Meta Platforms, Inc. and affiliates. */

#include "vmlinux.h"
#include <bpf/bpf_helpers.h>

/*
 * Declare the kfunc here because the pinned vmlinux.h revision predates
 * kernel commit d806f3101276 ("bpf: Migrate bpf_stream_vprintk() to
 * KF_IMPLICIT_ARGS"), which renamed bpf_stream_vprintk_impl back to
 * bpf_stream_vprintk and dropped the aux argument.
 */
extern int bpf_stream_vprintk(int stream_id, const char *fmt__str,
                              const void *args, __u32 len__sz) __weak __ksym;

/*
 * Trigger writing of some messages to stdout & stderr streams.
 */
SEC("syscall")
int trigger_streams(void *ctx)
{
    bpf_stream_printk(1, "stdout");
    bpf_stream_printk(2, "stderr");
    return 0;
}

char LICENSE[] SEC("license") = "GPL";
