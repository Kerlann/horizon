#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <errno.h>
#include <netdb.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <unistd.h>
#include <string.h>

/* Per-invocation append-only channel, inherited by child processes. A refusal is
 * recorded even if the test handles the error. Never permit a lost diagnostic. */
static void record(const char *operation) {
    const char *path = getenv("HORIZON_NETWORK_GUARD_LOG");
    if (!path) _exit(125);
    int fd = open(path, O_WRONLY | O_APPEND | O_CLOEXEC);
    if (fd < 0) _exit(125);
    size_t n = strlen(operation);
    if (write(fd, operation, n) != (ssize_t)n || close(fd)) _exit(125);
}
__attribute__((constructor)) static void loaded(void) { record("loaded\n"); }
static int inet(int family) { return family == AF_INET || family == AF_INET6; }
int socket(int family, int type, int protocol) {
    if (inet(family)) { record("socket\n"); errno = EACCES; return -1; }
    int (*real)(int, int, int) = dlsym(RTLD_NEXT, "socket");
    return real(family, type, protocol);
}
int bind(int fd, const struct sockaddr *address, socklen_t size) {
    if (address && inet(address->sa_family)) { record("bind\n"); errno = EACCES; return -1; }
    int (*real)(int, const struct sockaddr *, socklen_t) = dlsym(RTLD_NEXT, "bind");
    return real(fd, address, size);
}
int connect(int fd, const struct sockaddr *address, socklen_t size) {
    if (address && inet(address->sa_family)) { record("connect\n"); errno = EACCES; return -1; }
    int (*real)(int, const struct sockaddr *, socklen_t) = dlsym(RTLD_NEXT, "connect");
    return real(fd, address, size);
}
int getaddrinfo(const char *name, const char *service, const struct addrinfo *hints,
                struct addrinfo **result) {
    (void)name; (void)service; (void)hints; (void)result;
    record("getaddrinfo\n");
    return EAI_FAIL;
}
