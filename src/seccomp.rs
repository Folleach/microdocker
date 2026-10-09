// filter written in classic BPF (cBPF)
// https://man7.org/linux/man-pages/man2/seccomp.2.html
// https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html

use std::{io::Error, mem::offset_of};

use libc::{seccomp_data, sock_filter, sock_fprog};

// https://github.com/torvalds/linux/blob/v7.0/include/uapi/linux/audit.h#L396
#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64

pub fn restrict_syscalls() -> Result<(), String> {
    // https://man.openbsd.org/bpf.4#Filter_machine
    let load = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;     // A = 32 bit word at offset k
    let jump_eq = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16; // if A == k { skip jt } else { skip jf }
    let ret = (libc::BPF_RET | libc::BPF_K) as u16;                     // return k

    let mut filter = [
        /* 0 */ sock_filter { code: load,    jt: 0, jf: 0, k: offset_of!(seccomp_data, arch) as u32 }, // load architecture to accumulator (A)
        /* 1 */ sock_filter { code: jump_eq, jt: 1, jf: 0, k: AUDIT_ARCH }, // our arch ? jump to 3 (skip 1) : jump to 2 (do not skip anything)
        /* 2 */ sock_filter { code: ret,     jt: 0, jf: 0, k: libc::SECCOMP_RET_KILL_PROCESS }, // kill process

        /* 3 */ sock_filter { code: load,    jt: 0, jf: 0, k: offset_of!(seccomp_data, nr) as u32 }, // load syscall number to A
        /* 4 */ sock_filter { code: jump_eq, jt: 1, jf: 0, k: libc::SYS_chroot as u32 },  // chroot ? jump to 6 : jump to 5
        /* 5 */ sock_filter { code: jump_eq, jt: 0, jf: 1, k: libc::SYS_chdir as u32 }, // chdir ? jump to 6 : jump to 7
        /* 6 */ sock_filter { code: ret,     jt: 0, jf: 0, k: libc::SECCOMP_RET_ERRNO | libc::EPIPE as u32 }, // return funny error (broken pipe)
        /* 7 */ sock_filter { code: ret,     jt: 0, jf: 0, k: libc::SECCOMP_RET_ALLOW }, // allow (default action)
    ];

    let program = sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr()
    };

    unsafe {
        if libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program as *const sock_fprog) == -1 {
            return Err(format!("failed to install seccomp filter: {}", Error::last_os_error()));
        }
    }

    return Ok(());
}
