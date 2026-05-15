/* Test stubs for missing C symbols used by syscall module */
int lwip_socket(int domain, int type, int protocol) { return -1; }
int lwip_bind(int s, const void *name, unsigned namelen) { return -1; }
int lwip_listen(int s, int backlog) { return -1; }
int lwip_accept(int s, void *addr, unsigned *addrlen) { return -1; }
int lwip_connect(int s, const void *name, unsigned namelen) { return -1; }
long lwip_send(int s, const void *data, unsigned long size, int flags) { return -1; }
long lwip_recv(int s, void *mem, unsigned long len, int flags) { return -1; }
int lwip_close(int s) { return -1; }
