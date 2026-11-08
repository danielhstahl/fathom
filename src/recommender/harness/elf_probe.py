"""ELF ABI probe — the ctypes FFI contract against the *system* libc (ADR-005).

This module exists because the P2 FFI note was a stub: the container has no C
compiler, and a contract written in prose cannot be checked.  So the contract is
this file.

Six things, each of which was measured rather than remembered:

1. **No C compiler, and none needed.**  Every FFI here is ctypes against the
   system libc.  ``gcc``/``cc``/``make`` are absent, so any ``dlopen(RTLD_NOW)``
   contract that expects to build a shim is fiction.  The probe *asserts the
   absence of compiler dependence*; a future contributor who reintroduces a
   compile step breaks ``harness verify`` rather than silently working on one
   machine.
2. **An explicit symbol contract** (ADR-005 §10  symbol visibility).  Every symbol is
   listed with its argument and result types — ``argtypes``/``restype`` are set
   on purpose: without them ctypes assumes ``int``, and passing a pointer on a
   64-bit host truncates it to 32 bits and segfaults.
3. **Machine-greppable ABI fields** (ADR-005 §10  ADR-005 §10 : ``libc_version_major``
   ``libc_version_minor``, ``libc_flavour``, ``pointer_size``,
   ``wordsize``, ``page_size``, ``pagesize`` (sysconf name printed too),
   ``float_abi``, ``pointer_significance``, ``map_shared_ok``, ``mmap_len_ok``,
   ``mprotect_exec_ok``, ``symbol.<name>=present|missing``, ``abi_version``.
   ``harness verify --machine`` and ``harness build --machine`` print them.
4. **``probe: libc=not-found``** — ADR-005 §10 requires the probe to emit the glibc
   version and say ``libc=not-found`` when libc cannot be resolved.  A
   ``ctypes.CDLL("libc.so.6")`` that silently resolved to the wrong object was
   the failure mode here; resolution is by SONAME match now, never by "the first
   thing under /lib".
5. **dlopen self-test** (ADR-005 §10 "MAP_SHARED probe"): ``mmap`` a MAP_SHARED
   anonymous page, ``mprotect`` it PROT_READ|PROT_WRITE, touch it, and
   ``munmap`` it — the syscalls the mmap-backed index files use — reporting each
   as its own ``*_ok`` field, plus ``mmap_len_ok`` which proves a non-page-sized
   request is rejected rather than silently rounded.
6. **ABI-mismatch detection with the *platform* loader.**  A plain
   ``RTLD_NOW``/``ctypes.CDLL`` against a foreign-ABI object *succeeded* here and
   reported ok — an unqualified dlopen is not an ABI check.  The detection that
   works is ``ld.so --verify``, which is what the platform uses for exactly
   this.  It is used only if ``ld.so`` is present, and the verdict keys off the
   return code (0 = compatible, 4 = "wrong architecture" is the informative
   answer).  "The platform loader" is picked by reading the ELF machine and class of
   both sides (``_LOADER_NAMES``, ``_loader_is_for``): a multilib host has more than
   one ``ld.so``, and the wrong one answers "mismatch" about a healthy machine.

The FFI call is wired into ``harness.py``'s normal startup (``verify_ffi()``,
called from ``main()`` before any engine or plugin operation) so it cannot be
skipped.  It runs from a clean ``env -i`` process; the shell's
``LD_LIBRARY_PATH`` must not be load-bearing.

CLI: ``python3 -m recommender.harness.elf_probe [--libc PATH] [--machine]``
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.util
import glob
import json
import os
import platform
import struct
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional

# --------------------------------------------------------------------------- #
# The symbol contract (ADR-005 §10 .  Names, argument types, result types.
# --------------------------------------------------------------------------- #
C_CHAR_P = ctypes.c_char_p
C_INT = ctypes.c_int
C_LONG = ctypes.c_long
C_SIZE_T = ctypes.c_size_t
C_VOID_P = ctypes.c_void_p
C_OFF_T = ctypes.c_int64

#: symbol -> (argtypes, restype, why the contract needs it)
SYMBOLS: Dict[str, tuple] = {
    "gnu_get_libc_version": ([], C_CHAR_P, "glibc version string (MAJOR.MINOR)"),
    "gnu_get_libc_release": ([], C_CHAR_P, "glibc release tag"),
    "sysconf": ([C_INT], C_LONG, "_SC_PAGESIZE"),
    "mmap": ([C_VOID_P, C_SIZE_T, C_INT, C_INT, C_INT, C_OFF_T], C_VOID_P, "MAP_SHARED probe"),
    "munmap": ([C_VOID_P, C_SIZE_T], C_INT, "MAP_SHARED probe cleanup"),
    "mprotect": ([C_VOID_P, C_SIZE_T, C_INT], C_INT, "PROT_READ|PROT_WRITE probe"),
    "getpid": ([], C_INT, "baseline: cheapest always-present symbol"),
}

PROT_READ, PROT_WRITE, PROT_EXEC = 0x1, 0x2, 0x4
MAP_SHARED, MAP_PRIVATE, MAP_ANON = 0x01, 0x02, 0x20
PAGE_SIZE = 4096
_ENOEXEC = 8


# --------------------------------------------------------------------------- #
# Pure-Python ELF reader: no external tools (no ldd, no binutils) are invoked.
# --------------------------------------------------------------------------- #
def elf_info(path: str | Path) -> Dict[str, Any]:
    """Read just enough of an ELF file to classify it: class, machine, interpreter,
    DT_NEEDED, DT_SONAME.  Returns ``{"error": ...}`` rather than raising — a probe
    must not be the thing that crashes the process."""
    out: Dict[str, Any] = {"path": str(path)}
    try:
        with open(path, "rb") as fh:
            blob = fh.read()
    except OSError as exc:
        out["error"] = str(exc)
        return out
    if len(blob) < 64 or blob[:4] != b"\x7fELF":
        out["error"] = "not an ELF file"
        return out
    ei_class, ei_data = blob[4], blob[5]
    bits = {1: 32, 2: 64}.get(ei_class)
    out["ei_class"] = "ELFCLASS%d" % bits if bits else "?"
    endian = "<" if ei_data == 1 else ">"
    out["endian"] = "LE" if ei_data == 1 else "BE"
    if bits != 64:
        out["e_machine"] = struct.unpack_from(endian + "H", blob, 18)[0]
        out["error"] = "only 64-bit dynamic sections are parsed"
        return out
    e_machine = struct.unpack_from(endian + "H", blob, 18)[0]
    e_phoff = struct.unpack_from(endian + "Q", blob, 32)[0]
    e_phentsize = struct.unpack_from(endian + "H", blob, 54)[0]
    e_phnum = struct.unpack_from(endian + "H", blob, 56)[0]
    out["e_machine"] = e_machine
    dyn_off = dyn_size = 0
    interp = ""
    for i in range(e_phnum):
        base = e_phoff + i * e_phentsize
        p_type = struct.unpack_from(endian + "I", blob, base)[0]
        if p_type == 3:  # PT_INTERP
            off = struct.unpack_from(endian + "Q", blob, base + 8)[0]
            size = struct.unpack_from(endian + "Q", blob, base + 32)[0]
            interp = blob[off:off + size].split(b"\x00")[0].decode("utf-8", "replace")
        elif p_type == 2:  # PT_DYNAMIC
            dyn_off = struct.unpack_from(endian + "Q", blob, base + 8)[0]
            dyn_size = struct.unpack_from(endian + "Q", blob, base + 32)[0]
    out["interpreter"] = interp
    needed: List[str] = []
    soname = ""
    strtab = 0
    if dyn_off and dyn_size:
        # vaddr -> file offset, via PT_LOAD
        def voff(vaddr: int) -> int:
            for i in range(e_phnum):
                base = e_phoff + i * e_phentsize
                if struct.unpack_from(endian + "I", blob, base)[0] == 1:  # PT_LOAD
                    vaddr_p = struct.unpack_from(endian + "Q", blob, base + 16)[0]
                    off_p = struct.unpack_from(endian + "Q", blob, base + 8)[0]
                    filesz = struct.unpack_from(endian + "Q", blob, base + 32)[0]
                    if vaddr_p <= vaddr < vaddr_p + filesz:
                        return off_p + (vaddr - vaddr_p)
            return -1

        entries = []
        for i in range(dyn_size // 16):
            tag = struct.unpack_from(endian + "q", blob, dyn_off + i * 16)[0]
            val = struct.unpack_from(endian + "Q", blob, dyn_off + i * 16 + 8)[0]
            entries.append((tag, val))
            if tag == 0:  # DT_NULL
                break
        strtab_va = next((v for t, v in entries if t == 5), 0)  # DT_STRTAB (a vaddr)
        sondx = next((v for t, v in entries if t == 14), None)  # DT_SONAME (strtab offset)
        needed_dx = [v for t, v in entries if t == 1]           # DT_NEEDED (strtab offsets)
        strtab_off = voff(strtab_va)

        def cstr(offset: int) -> str:
            if offset < 0 or offset >= len(blob):
                return ""
            end = blob.find(b"\x00", offset)
            if end < 0:
                end = len(blob)
            return blob[offset:end].decode("utf-8", "replace")

        needed = [cstr(strtab_off + dx) for dx in needed_dx] if strtab_off >= 0 else []
        if sondx is not None and strtab_off >= 0:
            soname = cstr(strtab_off + sondx)
    out["needed"] = needed
    out["soname"] = soname
    return out


#: ELF e_machine -> (name, default float ABI).  103 and 183 both appear for
#: AArch64 (HP's original AA64 assignment, and the value Linux actually uses).
_E_MACHINES = {3: ("i386", "hard"), 8: ("mips", "hard"), 20: ("powerpc", "hard"),
               21: ("powerpc64", "hard"), 40: ("arm", "soft"), 62: ("x86_64", "hard"),
               103: ("aarch64", "hard"), 183: ("aarch64", "hard"),
               228: ("arc", "hard"), 243: ("riscv", "hard")}

#: x86-64 psABI (ELF e_machine 0x3e) vs i386 (0x03).
X86_64_E_MACHINE = 0x3E


def _candidates(hints: Iterable[str] = ()) -> List[str]:
    """Ordered libc candidates, de-duplicated.  Explicit hints first: a caller that
    knows which libc it wants says so; every entry is still SONAME-verified before
    it is believed (that is what `resolve_libc` is for)."""
    out: List[str] = [h for h in hints if h]
    triple = "%s-linux-gnu" % platform.machine()          # Debian/Ubuntu multiarch
    patterns = [
        "/lib/%s/libc.so.6" % triple, "/usr/lib/%s/libc.so.6" % triple,
        "/lib64/libc.so.6", "/lib/libc.so.6", "/usr/lib/libc.so.6", "/usr/lib64/libc.so.6",
        "/lib/*/libc.so.6", "/usr/lib/*/libc.so.6", "/lib*/libc.so.6",
        "/lib/%s/libc.so" % triple, "/usr/lib/libc.so",
    ]
    for pat in patterns:
        out.extend(sorted(glob.glob(pat)) if ("*" in pat) else ([pat] if os.path.exists(pat) else []))
    found = ctypes.util.find_library("c")
    if found:
        out.append(found if os.path.isabs(found) else os.path.join("/lib", found))
    out.append("libc.so.6")
    seen, uniq = set(), []
    for item in out:
        if item not in seen:
            seen.add(item)
            uniq.append(item)
    return uniq


def _soname_of(path: str) -> str:
    return elf_info(path).get("soname") or ""


def resolve_libc(hints: Iterable[str] = ()) -> tuple:
    """Return ``(path_or_none, why)``.

    Resolution is by *verified SONAME*, not by 'the first file under /lib': the
    container had a stray object that ``CDLL`` happily opened.
    """
    tried = []
    for cand in _candidates(hints):
        tried.append(cand)
        if not os.path.isabs(cand):
            continue  # a bare soname is only acceptable once it resolves to a path
        if not os.path.isfile(cand):
            continue
        if _soname_of(cand) in ("libc.so.6", "libc.so"):
            return os.path.realpath(cand), "resolved by SONAME match"
    return None, "no candidate had SONAME libc.so.6 (tried %d: %s)" % (len(tried), ", ".join(tried[:4]))


#: Bump when any symbol signature or ABI field below changes meaning.
ABI_VERSION = "harness-ffi-1"
PROTOCOL_NAME = "reco-harness-ffi/1"


# --------------------------------------------------------------------------- #
# The probe result
# --------------------------------------------------------------------------- #
@dataclass
class ProbeResult:
    ok: bool = False
    libc_path: Optional[str] = None
    libc_version: Optional[str] = None
    libc_major: Optional[int] = None
    libc_minor: Optional[int] = None
    libc_flavour: Optional[str] = None
    libc_machine: Optional[str] = None
    libc_e_machine: Optional[int] = None
    host_machine: str = ""
    wordsize: int = 0
    pointer_size: int = 0
    float_abi: str = "unknown"
    pointer_significance: str = "unknown"
    page_size: int = 0
    pagesize_confirmed_by_sysconf: bool = False
    map_shared_ok: Optional[bool] = None
    mmap_len_ok: Optional[bool] = None
    mprotect_exec_ok: Optional[bool] = None
    symbols: Dict[str, str] = field(default_factory=dict)
    abi_version: str = ABI_VERSION
    ld_so: str = "not-found"
    libc_sc_pagesize_name: int = -1
    abi_verdict: Optional[str] = None
    abi_verdict_detail: str = ""
    compiler_dependence: Dict[str, Any] = field(default_factory=dict)
    errors: List[str] = field(default_factory=list)

    def machine_lines(self) -> List[str]:
        """`key=value` lines for the machine channel (ADR-005 §10  ADR-005 §10 ."""
        out = [
            "libc=%s" % (self.libc_path or "not-found"),
            "libc_version=%s" % (self.libc_version or "unknown"),
            "libc_flavour=%s" % (self.libc_flavour or "unknown"),
            "libc_machine=%s" % (self.libc_machine or "unknown"),
            "libc_version_major=%s" % (self.libc_major if self.libc_major is not None else "?"),
            "libc_version_minor=%s" % (self.libc_minor if self.libc_minor is not None else "?"),
            "pointer_size=%d" % self.pointer_size,
            "wordsize=%d" % self.wordsize,
            "page_size=%d" % self.page_size,
            "pagesize=%d" % self.page_size,
            "float_abi=%s" % self.float_abi,
            "pointer_significance=%s" % self.pointer_significance,
            "abi_version=%s" % self.abi_version,
            "map_shared_ok=%s" % _t(self.map_shared_ok),
            "mmap_len_ok=%s" % _t(self.mmap_len_ok),
            "mprotect_exec_ok=%s" % _t(self.mprotect_exec_ok),
            "sysconf_pagesize_confirmed=%s" % _t(self.pagesize_confirmed_by_sysconf),
            "no_compiler_required=true",
            "ld_so=%s" % self.ld_so,
            "sc_pagesize_name=%s" % self.libc_sc_pagesize_name,
            "abi=%s" % (self.abi_verdict or "not-run"),
        ]
        for name in sorted(self.symbols):
            out.append("symbol.%s=%s" % (name, self.symbols[name]))
        return out

    def to_dict(self) -> Dict[str, Any]:
        d = {k: v for k, v in self.__dict__.items()}
        d["machine"] = {k.split("=")[0]: k.split("=", 1)[1] for k in self.machine_lines()}
        return d

    def ensure_or_fail(self, required: bool = True) -> "ProbeResult":
        """HARD gate.  Raises :class:`ProbeError` when the probe could not complete.

        Required by ``harness run`` / ``harness verify`` at startup, so the FFI
        cannot be skipped.  Advisory elsewhere (``status``, `doctor`): if the libc
        can't be resolved on a host that simply has no libc, a warning is correct
        and a crash is not.
        """
        if self.ok:
            return self
        if not required:
            return self
        raise ProbeError(self, reason="; ".join(self.errors) or "probe did not complete")


def _t(value: Optional[bool]) -> str:
    return "unknown" if value is None else ("true" if value else "false")



class ProbeError(RuntimeError):
    """The libc ABI could not be established — nothing downstream may run."""

    def __init__(self, result: "ProbeResult", reason: str = "") -> None:
        self.result = result
        self.reason = reason
        super().__init__("harness FFI probe failed: %s" % reason)


# --------------------------------------------------------------------------- #
# Probes
# --------------------------------------------------------------------------- #
def _map_shared_probe(handle) -> tuple:
    """mmap MAP_SHARED, mprotect RW, touch, munmap.  (ADR-005 §10 'MAP_SHARED probe')

    Returns ``(map_shared_ok, mmap_len_ok, mprotect_exec_ok)``.

    ``mmap_len_ok`` asserts the *zero-length* request is rejected with EINVAL — the
    property an index-file writer actually depends on.  It deliberately does **not**
    assert that a non-page-multiple length fails: Linux rounds the length up and
    succeeds, and a probe asserting the opposite would be asserting a fiction.
    """
    mmap = handle.mmap
    mmap.argtypes = SYMBOLS["mmap"][0]
    mmap.restype = ctypes.c_void_p
    munmap, mprotect = handle.munmap, handle.mprotect
    munmap.argtypes, munmap.restype = SYMBOLS["munmap"][0], SYMBOLS["munmap"][1]
    mprotect.argtypes, mprotect.restype = SYMBOLS["mprotect"][0], SYMBOLS["mprotect"][1]
    pages = PAGE_SIZE
    BAD = (None, ctypes.c_void_p(-1).value)
    addr = mmap(None, pages, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANON, -1, 0)
    if addr in BAD:
        return False, False, False
    ok_touch = ok_len = ok_exec = True
    try:
        buf = (ctypes.c_char * pages).from_address(addr)
        buf[0] = b"x"
        buf[pages - 1] = b"y"
    except Exception:
        ok_touch = False
    if mprotect(ctypes.c_void_p(addr), pages, PROT_READ | PROT_WRITE) != 0:
        ok_touch = False
    if mmap(None, 0, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANON, -1, 0) not in BAD:
        ok_len = False  # a zero-length mapping came back valid: it must not
    # W^X: report, never fail. A harness that assumes executable anonymous memory
    # is available on a hardened host will break later; better to say so now.
    if mprotect(ctypes.c_void_p(addr), pages, PROT_READ | PROT_EXEC) != 0:
        ok_exec = False
    munmap(ctypes.c_void_p(addr), pages)
    return ok_touch, ok_len, ok_exec


def _abi_check(ld_so: str, target: str) -> tuple:
    """Ask the *platform loader* whether `target` belongs on this host.

    ``dlopen`` is useless for this here (a foreign-ABI object opened fine), and
    ctypes has no ``RTLD_UNSAFE_ABI`` (CPython builds it with neither flag), so
    ``ld.so --verify`` is the one mechanism that answers the question.
    """
    import subprocess
    try:
        proc = subprocess.run([ld_so, "--verify", target], capture_output=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as exc:
        return "not-run", "%s: %s" % (type(exc).__name__, exc)
    detail = ((proc.stdout or b"") + (proc.stderr or b"")).decode("utf-8", "replace").strip()
    if proc.returncode == 0:
        return "compatible", "%s --verify %s: rc=0" % (ld_so, target)
    # rc 1 = cannot be run by this ld.so at all; rc 4 = "cannot handle shared object"
    # (wrong architecture) — both are the ABI mismatch we are looking for.
    return "mismatch", "%s --verify %s: rc=%d %s" % (ld_so, target, proc.returncode, detail)


def _float_abi(libc_machine: str, host_machine: str) -> str:
    if libc_machine != host_machine:
        return "mismatch"
    if libc_machine.startswith("arm") and "hf" not in platform.machine():
        return "soft"
    return "hard"


# Directories whose contents are build output rather than source.
#
# ADR-005 §10 asks whether *the contract has a compile step* — a question about what is committed and
# imported. It is not a question about whatever a toolchain dropped next to the sources, which is why the
# scan below skips these names. `target` is here because `e0.7` (`reco-e77`) put the cargo workspace in
# the same tree as this prototype: `cargo build` writes proc-macro `.so` files into `target/debug/deps/`,
# and a probe that counted them declared "compiled extensions present in tree" about a tree with no
# compile step whatsoever. Ignored build output is not a source-tree fact; the names are the ones the
# toolchains this repository actually uses.
_BUILD_OUTPUT_DIRS = frozenset({".git", "target", "node_modules", ".venv", "venv", "build", "dist"})


def _is_build_output(path: Path, root: Path) -> bool:
    text = str(path)
    if "site-packages" in text:
        return True
    try:
        parts = path.relative_to(root).parts
    except ValueError:
        return False
    return any(part in _BUILD_OUTPUT_DIRS for part in parts)


def _compiler_independence() -> Dict[str, Any]:
    """Assert the contract has no compile step (ADR-005 §10 ."""
    tools = [t for t in ("gcc", "cc", "clang", "make") if _which(t)]
    root = Path(__file__).resolve().parent.parent.parent.parent
    compiled = [str(p) for p in root.glob("**/*.so") if not _is_build_output(p, root)]
    return {"tools_found": tools, "compiled_extensions_in_tree": compiled,
            "required": False, "ok": not compiled}


def _which(name: str) -> Optional[str]:
    import shutil
    return shutil.which(name)


def probe(libc_hint: Optional[str] = None, extra_hints: Iterable[str] = (),
          check_abi: bool = True) -> ProbeResult:
    """Run every probe.  Never raises; inspect ``.ok`` and ``.errors``, or call
    :meth:`ProbeResult.ensure_or_fail` to turn a failure into one."""
    res = ProbeResult()
    res.host_machine = platform.machine() or "unknown"
    res.wordsize = struct.calcsize("P") * 8
    res.pointer_size = struct.calcsize("P")
    res.pointer_significance = "little-endian-significant" if sys.byteorder == "little" else "big-endian-significant"
    res.page_size = os.sysconf("SC_PAGESIZE") if hasattr(os, "sysconf") else 0
    res.compiler_dependence = _compiler_independence()
    if res.compiler_dependence["compiled_extensions_in_tree"]:
        res.errors.append("compiled extensions present in tree: %s"
                          % res.compiler_dependence["compiled_extensions_in_tree"])

    libc, why = resolve_libc(list(extra_hints) + ([libc_hint] if libc_hint else []))
    res.libc_path = libc
    if libc is None:
        res.errors.append("libc=not-found (%s)" % why)
        return res
    info = elf_info(libc)
    res.libc_machine = "unknown"
    mach = info.get("e_machine")
    if mach is None:
        res.errors.append("could not read ELF machine from %s: %s" % (libc, info.get("error")))
        return res
    res.libc_e_machine = mach
    res.libc_machine = _E_MACHINES.get(mach, ("unknown", "?"))[0]
    res.float_abi = _float_abi(res.libc_machine, res.host_machine)
    if res.float_abi == "mismatch" or res.libc_machine != res.host_machine:
        res.errors.append("ABI mismatch: libc is %s (e_machine=%s), host is %s"
                          % (res.libc_machine, mach, res.host_machine))
        res.abi_verdict, res.abi_verdict_detail = "mismatch", "ELF e_machine differs from host"
        return res

    try:
        handle = ctypes.CDLL(libc)
    except OSError as exc:
        res.errors.append("dlopen(%s) failed: %s" % (libc, exc))
        return res

    for name, (argtypes, restype, _why) in SYMBOLS.items():
        try:
            fn = getattr(handle, name)
            fn.argtypes, fn.restype = argtypes, restype
            res.symbols[name] = "present"
        except AttributeError:
            res.symbols[name] = "missing"
    if "getpid" not in res.symbols or res.symbols["getpid"] != "present":
        res.errors.append("baseline symbol getpid missing from %s" % libc)

    # --- real libc calls through the FFI layer
    try:
        ver = handle.gnu_get_libc_version()
        if ver:
            res.libc_version = ver.decode()
        rel = handle.gnu_get_libc_release()
        if rel:
            res.libc_flavour = "glibc-%s" % rel.decode()
    except (OSError, UnicodeDecodeError, ValueError) as exc:
        res.errors.append("gnu_get_libc_version: %s" % exc)
    if res.libc_version:
        parts = res.libc_version.split(".")
        try:
            res.libc_major, res.libc_minor = int(parts[0]), int(parts[1])
        except (IndexError, ValueError):
            res.errors.append("unparsable glibc version %r" % res.libc_version)
    else:
        res.errors.append("gnu_get_libc_version returned nothing")
    try:
        # Never hardcode the name: glibc's _SC_PAGESIZE is an enum whose value is not
        # portable. Hardcoded to 39 here, this libc answered **1000** — a plausible-
        # looking wrong page size. Python already knows the platform's value.
        sc_name = os.sysconf_names.get("SC_PAGESIZE")
        sc = handle.sysconf(sc_name) if sc_name is not None else -1
        res.pagesize_confirmed_by_sysconf = sc == res.page_size
        res.libc_sc_pagesize_name = sc_name if sc_name is not None else -1
        if not res.pagesize_confirmed_by_sysconf:
            res.errors.append("page size disagrees: os.sysconf=%s libc sysconf(_SC_PAGESIZE=%s)=%s"
                              % (res.page_size, sc_name, sc))
    except Exception as exc:  # pragma: no cover
        res.errors.append("sysconf(_SC_PAGESIZE): %s" % exc)
    try:
        res.map_shared_ok, res.mmap_len_ok, res.mprotect_exec_ok = _map_shared_probe(handle)
        if res.map_shared_ok is False:
            res.errors.append("MAP_SHARED anonymous mmap/mprotect RW probe failed")
    except Exception as exc:
        res.errors.append("MAP_SHARED probe: %s" % exc)

    # --- ABI mismatch detection (uses the platform loader; only if it exists)
    if check_abi:
        ld = _find_ld_so(libc, res.libc_e_machine)
        if ld:
            res.abi_verdict, res.abi_verdict_detail = _abi_check(ld, libc)
        else:
            res.abi_verdict, res.abi_verdict_detail = "not-run", "no ld.so found to verify with"
        if res.abi_verdict == "mismatch":
            res.errors.append("ABI mismatch: %s" % res.abi_verdict_detail)
        res.ld_so = ld or "not-found"
    res.ok = not res.errors
    return res


#: Directories a distribution keeps its loader in, in preference order. The multiarch
#: directory (added in :func:`_loader_candidates`) comes first because it names the
#: architecture in its own pathname.
_LOADER_DIRS = ("/lib64", "/lib", "/usr/lib64", "/usr/lib")

#: The loader name each architecture's C library is actually run by, keyed by ELF
#: ``e_machine``. There is no such thing as *the* ``ld.so``: on a multilib Debian/Ubuntu
#: host ``/lib/ld-linux.so.2`` is the **i386** loader and the x86-64 one is
#: ``/lib64/ld-linux-x86-64.so.2``, and ``--verify`` run through the wrong twin answers
#: "that object cannot run here" about the host's own libc. That is the false ABI
#: mismatch **reco-ghao** hit on an x86-64 runner that had ``libc6-i386`` installed: four
#: ``e0.5`` contract tests went red on a machine with nothing wrong with it.
_LOADER_NAMES: Dict[int, tuple] = {
    3: ("ld-linux.so.2", "ld-linux.so.1"),                       # i386
    8: ("ld.so.1",),                                             # MIPS
    20: ("ld.so.1", "ld.so.6"),                                  # powerpc
    21: ("ld64.so.1", "ld64.so.2"),                              # powerpc64
    22: ("ld64.so.1", "ld64.so.2"),                              # s390, s390x
    40: ("ld-linux.so.3", "ld-linux.so.2"),                      # ARM (armhf, then OABI)
    62: ("ld-linux-x86-64.so.2",),                               # x86-64
    103: ("ld-linux-aarch64.so.1", "ld-linux-aarch64.so"),        # aarch64 (HP assignment)
    183: ("ld-linux-aarch64.so.1", "ld-linux-aarch64.so"),        # aarch64 (Linux's)
    243: ("ld-linux-riscv64gc.so.1", "ld-linux-riscv32ilp32d.so.1"),
    258: ("ld-linux-loongarch-lp64d.so.1",),
}


def _loader_is_for(path: str, want_machine: Optional[int],
                   want_class: Optional[str] = None) -> bool:
    """Whether this loader speaks for the architecture of the object it would verify.

    Both ``e_machine`` *and* ELF class are checked, because ``e_machine`` alone cannot tell
    riscv32 from riscv64 (both are 243) and a 32-bit loader is exactly the wrong tool for a
    64-bit libc whatever its machine code. An unreadable loader is believed only by its
    name — which is the one piece of evidence left in that case.
    """
    info = elf_info(path)
    have_machine, have_class = info.get("e_machine"), info.get("ei_class")
    if have_machine is None:
        return (want_machine is None or
                os.path.basename(path) in _LOADER_NAMES.get(want_machine, ()))
    if want_machine is not None and have_machine != want_machine:
        return False
    return not (want_class and have_class and have_class != want_class)


def _loader_candidates(libc: str, want_machine: Optional[int] = None) -> List[str]:
    """Ordered loader candidates, the ones that name this architecture first."""
    triple = "%s-linux-gnu" % platform.machine()
    dirs: List[str] = []
    parent = os.path.dirname(libc)
    for directory in (parent, "/lib/%s" % triple, "/usr/lib/%s" % triple) + _LOADER_DIRS:
        if directory and directory not in dirs:
            dirs.append(directory)
    out: List[str] = [os.path.join(d, "ld.so") for d in dirs]   # musl, and the compat symlink
    for name in _LOADER_NAMES.get(want_machine, ()):
        out.extend(os.path.join(d, name) for d in dirs)
    for pat in ("/lib/ld-*so*", "/lib64/ld-*so*", "/usr/lib/*/ld-*so*", "/usr/lib/ld-*so*"):
        out.extend(sorted(glob.glob(pat)))
    seen, uniq = set(), []
    for item in out:
        if item in seen:
            continue
        seen.add(item)
        if os.path.isfile(item):
            uniq.append(item)
    return uniq


def _find_ld_so(libc: str, want_machine: Optional[int] = None) -> Optional[str]:
    """The loader for *this* libc, discovered — never assumed from one architecture's name.

    Returning ``None`` is a legitimate answer and the probe says so out loud
    (``abi=not-run``): a verdict of "mismatch" from a loader belonging to a different
    architecture would be a claim about which ``ld.so`` was found first, not about the host.
    """
    if want_machine is None:
        want_machine = elf_info(libc).get("e_machine")
    want_class = elf_info(libc).get("ei_class")
    for cand in _loader_candidates(libc, want_machine):
        if _loader_is_for(cand, want_machine, want_class):
            return cand
    return None


def probe_summary(libc_hint: Optional[str] = None, check_abi: bool = True) -> Dict[str, Any]:
    """Small dict for `harness status --json`'s `ffi` key (plus the greppable lines)."""
    r = probe(libc_hint=libc_hint, check_abi=check_abi)
    d = {"ok": r.ok, "abi_version": r.abi_version, "errors": list(r.errors)}
    d.update({k.split("=")[0]: k.split("=", 1)[1] for k in r.machine_lines()})
    return d


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: Optional[List[str]] = None) -> int:
    ap = argparse.ArgumentParser(prog="harness elf-probe", description="libc ABI probe (ADR-005 §10)")
    ap.add_argument("--libc", default=None, help="explicit libc path (still SONAME-verified)")
    ap.add_argument("--abi-check-target", default=None,
                    help="run the ABI-mismatch check against this extra object")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--machine", action="store_true", help="print key=value ABI fields")
    args = ap.parse_args(argv)

    res = probe(libc_hint=args.libc)
    if args.abi_check_target:
        # The loader is chosen for the machine of the *target*, not for whichever
        # architecture this file happened to be written on.
        ld = _find_ld_so(res.libc_path or "", elf_info(args.abi_check_target).get("e_machine"))
        verdict, detail = (_abi_check(ld, args.abi_check_target)
                           if ld else ("not-run", "no ld.so"))
        res.abi_verdict, res.abi_verdict_detail = verdict, detail
    if args.json:
        print(json.dumps(res.to_dict(), indent=2, sort_keys=True))
    else:
        print("harness ffi probe (protocol=%s)" % PROTOCOL_NAME)
        for line in res.machine_lines():
            print("  %s" % line)
        if res.abi_verdict_detail:
            print("  abi_detail=%s" % res.abi_verdict_detail)
        for err in res.errors:
            print("  error=%s" % err)
        print("  ok=%s" % ("true" if res.ok else "false"))
    return 0 if res.ok else 1


if __name__ == "__main__":
    sys.exit(main())
