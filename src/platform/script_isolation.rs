//! Shared Linux x86_64 policy for fresh single-threaded page and research workers.
//! Install before reading untrusted input. No new permissions for engine probes.
use std::io;

const MEMORY_LIMIT: libc::rlim_t = 256 * 1024 * 1024;

fn set_limit(resource: u32, value: libc::rlim_t) -> Result<(), String> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: limit is initialized and valid for this synchronous OS call.
    if unsafe { libc::setrlimit(resource as _, &limit) } != 0 {
        return Err(format!(
            "setrlimit({resource}): {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}
pub(super) fn install_isolation() -> Result<(), String> {
    // close_range is required, not approximated by a guessed fd ceiling.
    // SAFETY: this entrypoint is a fresh, single-threaded child; descriptors
    // 0/1/2 are retained as the only intended communication endpoints.
    if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } != 0 {
        return Err(format!("close_range: {}", io::Error::last_os_error()));
    }
    set_limit(libc::RLIMIT_AS as _, MEMORY_LIMIT)?;
    set_limit(libc::RLIMIT_CPU as _, 1)?;
    set_limit(libc::RLIMIT_FSIZE as _, 0)?;
    set_limit(libc::RLIMIT_CORE as _, 0)?;
    set_limit(libc::RLIMIT_NOFILE as _, 3)?;
    // SAFETY: these prctl operations apply only to this dedicated child.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!("no_new_privs: {}", io::Error::last_os_error()));
    }
    let mut filter = filter();
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    if unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) } != 0 {
        return Err(format!("seccomp: {}", io::Error::last_os_error()));
    }
    Ok(())
}

fn statement(code: u32, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    }
}
fn jump(code: u32, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}
pub(super) fn filter() -> Vec<libc::sock_filter> {
    const ARCH_X86_64: u32 = 0xc000_003e;
    let load = libc::BPF_LD | libc::BPF_W | libc::BPF_ABS;
    let eq = libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K;
    let has = libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K;
    let ret = libc::BPF_RET | libc::BPF_K;
    let deny = statement(ret, libc::SECCOMP_RET_ERRNO | libc::EPERM as u32);
    let allow = statement(ret, libc::SECCOMP_RET_ALLOW);
    // seccomp_data offsets: nr=0, arch=4, args=16 (six u64 arguments).
    let mut code = vec![
        statement(load, 4),
        jump(eq, ARCH_X86_64, 1, 0),
        statement(ret, libc::SECCOMP_RET_KILL_PROCESS),
        statement(load, 0),
    ];
    let mut guarded = |syscall: libc::c_long, body: Vec<libc::sock_filter>| {
        code.push(jump(eq, syscall as u32, 0, body.len() as u8));
        code.extend(body);
    };
    guarded(
        libc::SYS_read,
        vec![
            statement(load, 20),
            jump(eq, 0, 1, 0),
            deny,
            statement(load, 16),
            jump(eq, 0, 0, 1),
            allow,
            deny,
        ],
    );
    guarded(
        libc::SYS_write,
        vec![
            statement(load, 20),
            jump(eq, 0, 1, 0),
            deny,
            statement(load, 16),
            jump(eq, 1, 1, 0),
            jump(eq, 2, 0, 1),
            allow,
            deny,
        ],
    );
    // mmap must be anonymous, fd=-1, and non-executable. Existing process
    // memory is private; no file/socket descriptor mapping is accepted.
    guarded(
        libc::SYS_mmap,
        vec![
            statement(load, 32),
            jump(has, libc::PROT_EXEC as u32, 0, 1),
            deny,
            statement(load, 40),
            jump(has, libc::MAP_ANONYMOUS as u32, 1, 0),
            deny,
            // The kernel interprets fd as a signed 32-bit int; x86_64 C ABI
            // callers may zero-extend or sign-extend -1 in its register.
            statement(load, 48),
            jump(eq, u32::MAX, 1, 0),
            deny,
            allow,
        ],
    );
    guarded(
        libc::SYS_mprotect,
        vec![
            statement(load, 32),
            jump(has, libc::PROT_EXEC as u32, 0, 1),
            deny,
            allow,
        ],
    );
    for syscall in [
        libc::SYS_close,
        libc::SYS_brk,
        libc::SYS_munmap,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_futex,
        libc::SYS_clock_gettime,
        libc::SYS_gettimeofday,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        libc::SYS_sched_yield,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_getrandom,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ] {
        code.push(jump(eq, syscall as u32, 0, 1));
        code.push(allow);
    }
    // Includes open/openat/openat2, socket/connect, clone/fork/vfork,
    // execve/execveat, ptrace, new descriptors, prctl and rlimit changes.
    // x32 syscall numbers also fall through to denial; no second ABI exists.
    code.push(deny);
    code
}
