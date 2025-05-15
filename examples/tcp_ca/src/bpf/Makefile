BPFTOOL ?= /home/deso/local/opt/libbpf-bootstrap/examples/c/.output/bpftool/bootstrap/bpftool

tcp_ca.skel.h: tcp_ca.bpf.o
	$(BPFTOOL) gen skeleton $< > $@

tcp_ca.bpf.o: tcp_ca.tmp.bpf.o
	$(BPFTOOL) gen object $@ $<

tcp_ca.tmp.bpf.o:
	clang -g -O2 -target bpf -D__TARGET_ARCH_x86 -I/home/deso/local/opt/libbpf-bootstrap/libbpf/include/uapi -I/home/deso/local/opt/libbpf-bootstrap/vmlinux.h/include/x86 -I/home/deso/local/opt/libbpf-bootstrap/blazesym/capi/include -I/home/deso/local/opt/libbpf-bootstrap/examples/c/.output/bpftool/bootstrap/libbpf/include -c tcp_ca.bpf.c -o $@
