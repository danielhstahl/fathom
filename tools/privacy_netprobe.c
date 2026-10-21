/* An egress tripwire for `tools/check_privacy.py` (ticket `e6.8`, ADR-002 §8.3).
 *
 * Compiled to a shared object and pulled into the fathom process with `LD_PRELOAD`. It does two things,
 * and only two: it *records* every attempt to reach another host, and it *refuses* that attempt. It never
 * lets a call through that it has logged, because a tripwire that also completes the circuit tells you
 * nothing about whether the building was entered.
 *
 * Scope, and why it is drawn this way:
 *
 *   * `AF_INET` / `AF_INET6` to a non-loopback address — logged and refused with `EACCES`. This is the
 *     "reached another machine" the privacy claim is about.
 *   * Loopback (127.0.0.0/8, ::1) — passed through. A test harness, an editor socket or a local proxy is
 *     not egress, and refusing it would make the tool unable to run at all on a machine with a local
 *     resolver. The count is separate so a caller can see both numbers.
 *   * `AF_UNIX`, `AF_UNIX`-style netlink, and anything that is not an internet family — passed through.
 *     A socket that cannot leave the host is not what the claim is about; blocking netlink would break
 *     name resolution machinery before it ever got to the network.
 *   * `getaddrinfo` — logged and failed with `EAI_FAIL`. A resolver-only leak is still a leak (the
 *     hostname is the leak), and a client that resolves before it connects is the common shape, so
 *     catching the lookup also catches a client whose `connect` this shim cannot reach.
 *
 * The log is one line per attempt, to the file named by `$NETPROBE_LOG`:
 *
 *     attempt <syscall> family=<af> addr=<address> port=<n> host=<name>
 *     local   <syscall> family=<af> addr=<address> port=<n>
 *
 * `local` lines are passed-through loopback traffic, reported so a reader can tell "nothing happened"
 * from "only localhost happened". This file has no build step of its own: `tools/check_privacy.py`
 * compiles it into a scratch directory with the system C compiler, which is the only caller and the only
 * reader of the format above.
 *
 * Written in C rather than Rust because it must load into a process that is already running and must not
 * link anything the target does not already have: `dlsym(RTLD_NEXT, ...)` plus libc, nothing else.
 */

#define _GNU_SOURCE
#include <arpa/inet.h>
#include <dlfcn.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

static FILE *probe_log(void) {
    const char *path = getenv("NETPROBE_LOG");
    if (path == NULL || path[0] == '\0') {
        return stderr; /* still logged: silence is how a tripwire goes unnoticed */
    }
    /* Append, never truncate: every attempt counts, including the ones from a previous command in a
     * multi-command run. A log that cannot be opened is still written to stderr rather than dropped:
     * a silently missing log is indistinguishable from a clean run, which is the worst possible pair. */
    FILE *handle = fopen(path, "ae");
    return handle != NULL ? handle : stderr;
}

static void note(const char *kind, const char *syscall, const char *fmt, ...) {
    FILE *handle = probe_log();
    if (handle == NULL) {
        return;
    }
    fprintf(handle, "%s %s", kind, syscall);
    va_list args;
    va_start(args, fmt);
    vfprintf(handle, fmt, args);
    va_end(args);
    fputc('\n', handle);
    fflush(handle);
    if (handle != stderr) {
        fclose(handle);
    }
}

static void describe(const struct sockaddr *addr, char *out, size_t len, int *loopback) {
    out[0] = '\0';
    *loopback = 0;
    if (addr == NULL) {
        snprintf(out, len, " family=none");
        return;
    }
    char host[NI_MAXHOST] = "";
    if (addr->sa_family == AF_INET) {
        const struct sockaddr_in *v4 = (const struct sockaddr_in *)addr;
        inet_ntop(AF_INET, &v4->sin_addr, host, sizeof host);
        snprintf(out, len, " family=inet addr=%s port=%u", host, (unsigned)ntohs(v4->sin_port));
        *loopback = (ntohl(v4->sin_addr.s_addr) & 0xff000000u) == 0x7f000000u;
    } else if (addr->sa_family == AF_INET6) {
        const struct sockaddr_in6 *v6 = (const struct sockaddr_in6 *)addr;
        inet_ntop(AF_INET6, &v6->sin6_addr, host, sizeof host);
        snprintf(out, len, " family=inet6 addr=%s port=%u", host, (unsigned)ntohs(v6->sin6_port));
        *loopback = IN6_IS_ADDR_LOOPBACK(&v6->sin6_addr);
    } else {
        snprintf(out, len, " family=other(%d)", (int)addr->sa_family);
        *loopback = 1; /* not an internet family: nothing here can leave this host */
    }
}

int connect(int sockfd, const struct sockaddr *addr, socklen_t addrlen) {
    static int (*real)(int, const struct sockaddr *, socklen_t);
    if (real == NULL) {
        real = dlsym(RTLD_NEXT, "connect");
    }
    char detail[256];
    int loopback = 0;
    describe(addr, detail, sizeof detail, &loopback);
    if (!loopback) {
        note("attempt", "connect", "%s", detail);
        errno = EACCES;
        return -1;
    }
    note("local", "connect", "%s", detail);
    return real(sockfd, addr, addrlen);
}

int bind(int sockfd, const struct sockaddr *addr, socklen_t addrlen) {
    static int (*real)(int, const struct sockaddr *, socklen_t);
    if (real == NULL) {
        real = dlsym(RTLD_NEXT, "bind");
    }
    char detail[256];
    int loopback = 0;
    describe(addr, detail, sizeof detail, &loopback);
    if (!loopback) {
        /* A listener on a routable address is as much a privacy event as an outbound call, and
         * `net-http-transport` is gated for exactly that reason (ADR-002 §6.3). */
        note("attempt", "bind", "%s", detail);
        errno = EACCES;
        return -1;
    }
    return real(sockfd, addr, addrlen);
}

ssize_t sendto(int sockfd, const void *buf, size_t len, int flags,
               const struct sockaddr *addr, socklen_t addrlen) {
    static ssize_t (*real)(int, const void *, size_t, int, const struct sockaddr *, socklen_t);
    if (real == NULL) {
        real = dlsym(RTLD_NEXT, "sendto");
    }
    if (addr != NULL) {
        char detail[256];
        int loopback = 0;
        describe(addr, detail, sizeof detail, &loopback);
        if (!loopback) {
            /* A datagram needs no `connect`, so a socket-only tripwire would miss it entirely. */
            note("attempt", "sendto", "%s", detail);
            errno = EACCES;
            return -1;
        }
    }
    return real(sockfd, buf, len, flags, addr, addrlen);
}

int getaddrinfo(const char *node, const char *service, const struct addrinfo *hints,
                struct addrinfo **res) {
    static int (*real)(const char *, const char *, const struct addrinfo *, struct addrinfo **);
    if (real == NULL) {
        real = dlsym(RTLD_NEXT, "getaddrinfo");
    }
    /* Numeric hosts are not lookups: `inet_pton` answers them without asking anyone, and the address is
     * checked at `connect`/`bind`/`sendto` anyway. Only a *name* is a question about somebody else's
     * address book, and only a name is refused here. `localhost` is the one name that cannot identify a
     * remote machine. */
    int ipv4 = 0;
    int ipv6 = 0;
    if (node != NULL) {
        struct in_addr v4;
        struct in6_addr v6;
        ipv4 = inet_pton(AF_INET, node, &v4) == 1;
        ipv6 = inet_pton(AF_INET6, node, &v6) == 1;
    }
    if (node == NULL || node[0] == '\0' || ipv4 || ipv6 || strcmp(node, "localhost") == 0 ||
        strcmp(node, "ip6-localhost") == 0) {
        return real(node, service, hints, res);
    }
    note("attempt", "getaddrinfo", " host=%s service=%s", node, service ? service : "-");
    return EAI_FAIL;
}
