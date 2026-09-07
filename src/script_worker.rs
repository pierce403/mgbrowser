//! One document's scripts run in a fresh, restricted child of this executable.
//!
//! This boundary is Linux/x86_64 research containment, not a sandbox for the
//! browser's renderer or network process. The child reads no page data until
//! limits, descriptor closure and seccomp are installed successfully.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use linux::{execute, selftest, worker_entry};

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn execute(_: mg_deps::js_browser::Request) -> Result<mg_deps::js_browser::Reply, String> {
    Err("Script-worker isolation is only implemented for Linux x86_64".into())
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn worker_entry() -> ! {
    std::process::exit(78)
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn selftest() -> Result<(), String> {
    Err("Script-worker isolation selftest requires Linux x86_64".into())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use mg_deps::js_browser::{Reply, Request};
    use serde::Serialize;
    use std::{
        io::{self, Read, Write},
        os::{fd::AsRawFd, unix::process::ExitStatusExt},
        process::{Child, Command, ExitStatus, Stdio},
        time::{Duration, Instant},
    };

    const INPUT_LIMIT: usize = 2 * 1024 * 1024;
    const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
    const WALL_LIMIT: Duration = Duration::from_secs(2);
    const MEMORY_LIMIT: libc::rlim_t = 256 * 1024 * 1024;
    const SETUP_EXIT: i32 = 73;
    const REQUEST_EXIT: i32 = 74;

    struct LimitedBuffer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for LimitedBuffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::other("Script-worker JSON byte limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, String> {
        let mut output = LimitedBuffer {
            bytes: Vec::new(),
            limit,
        };
        serde_json::to_writer(&mut output, value).map_err(|e| e.to_string())?;
        Ok(output.bytes)
    }

    /// The deadline includes encoding, startup, pipe transfer and execution.
    pub fn execute(request: Request) -> Result<Reply, String> {
        let deadline = Instant::now() + WALL_LIMIT;
        let input = encode(&request, INPUT_LIMIT)
            .map_err(|e| format!("Script-worker request exceeds 2 MiB or is invalid: {e}"))?;
        let outcome = run(&input, None, deadline).map_err(|e| e.message)?;
        if !outcome.status.success() {
            let detail = serde_json::from_slice::<serde_json::Value>(&outcome.output)
                .ok()
                .and_then(|v| {
                    v.get("worker_error")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                });
            return Err(
                detail.unwrap_or_else(|| format!("Script worker terminated: {}", outcome.status))
            );
        }
        serde_json::from_slice(&outcome.output)
            .map_err(|e| format!("Invalid script-worker reply: {e}"))
    }

    struct OwnedChild {
        child: Child,
        reaped: bool,
    }
    impl OwnedChild {
        fn stop(&mut self) {
            if !self.reaped {
                let _ = self.child.kill();
                if self.child.wait().is_ok() {
                    self.reaped = true;
                }
            }
        }
    }
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            self.stop();
        }
    }
    #[derive(Debug, PartialEq, Eq)]
    enum FailureKind {
        Other,
        Deadline,
        OutputLimit,
    }
    struct Failure {
        kind: FailureKind,
        message: String,
        pid: Option<u32>,
    }
    struct Outcome {
        status: ExitStatus,
        output: Vec<u8>,
        pid: u32,
    }
    fn failure(kind: FailureKind, message: impl Into<String>, pid: Option<u32>) -> Failure {
        Failure {
            kind,
            message: message.into(),
            pid,
        }
    }

    fn nonblocking(fd: i32) -> io::Result<()> {
        // SAFETY: fd is a live, owned ChildStdin/ChildStdout descriptor. fcntl
        // touches its flags; no pointer or memory is passed to the kernel.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn run(input: &[u8], probe: Option<&str>, deadline: Instant) -> Result<Outcome, Failure> {
        if input.len() > INPUT_LIMIT {
            return Err(failure(
                FailureKind::Other,
                "Script-worker request exceeds 2 MiB",
                None,
            ));
        }
        let executable = std::env::current_exe().map_err(|e| {
            failure(
                FailureKind::Other,
                format!("Cannot locate script-worker executable: {e}"),
                None,
            )
        })?;
        let mut command = Command::new(executable);
        command
            .arg("--script-worker")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(probe) = probe {
            command.arg(format!("--probe={probe}"));
        }
        let child = command.spawn().map_err(|e| {
            failure(
                FailureKind::Other,
                format!("Cannot start script worker: {e}"),
                None,
            )
        })?;
        let pid = child.id();
        let mut owned = OwnedChild {
            child,
            reaped: false,
        };
        let result = (|| {
            let mut stdin = owned.child.stdin.take();
            let mut stdout = owned.child.stdout.take().ok_or_else(|| {
                failure(
                    FailureKind::Other,
                    "Script-worker stdout pipe missing",
                    Some(pid),
                )
            })?;
            nonblocking(stdout.as_raw_fd())
                .and_then(|_| {
                    nonblocking(
                        stdin
                            .as_ref()
                            .ok_or_else(|| io::Error::other("stdin pipe missing"))?
                            .as_raw_fd(),
                    )
                })
                .map_err(|e| {
                    failure(
                        FailureKind::Other,
                        format!("Cannot configure worker pipes: {e}"),
                        Some(pid),
                    )
                })?;
            let mut sent = 0;
            let mut output = Vec::new();
            let mut eof = false;
            let mut status = None;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let mut progressed = false;
                if Instant::now() >= deadline {
                    return Err(failure(
                        FailureKind::Deadline,
                        "Script worker exceeded the 2-second wall deadline; child killed and reaped",
                        Some(pid),
                    ));
                }
                if status.is_none() {
                    status = owned.child.try_wait().map_err(|e| {
                        failure(
                            FailureKind::Other,
                            format!("Cannot observe script worker: {e}"),
                            Some(pid),
                        )
                    })?;
                    if status.is_some() {
                        owned.reaped = true;
                    }
                }
                if sent == input.len() || status.is_some() {
                    stdin.take(); // EOF completes the one-request protocol.
                }
                if let Some(pipe) = stdin.as_mut() {
                    match pipe.write(&input[sent..(sent + buffer.len()).min(input.len())]) {
                        Ok(0) => {
                            return Err(failure(
                                FailureKind::Other,
                                "Script-worker input pipe stopped accepting bytes",
                                Some(pid),
                            ));
                        }
                        Ok(n) => {
                            sent += n;
                            progressed = true;
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                            ) => {}
                        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                            stdin.take();
                        }
                        Err(e) => {
                            return Err(failure(
                                FailureKind::Other,
                                format!("Script-worker input failed: {e}"),
                                Some(pid),
                            ));
                        }
                    }
                }
                if !eof {
                    match stdout.read(&mut buffer) {
                        Ok(0) => {
                            eof = true;
                            progressed = true;
                        }
                        Ok(n) => {
                            if n > OUTPUT_LIMIT.saturating_sub(output.len()) {
                                return Err(failure(
                                    FailureKind::OutputLimit,
                                    "Script worker exceeded the 4 MiB output limit; child killed and reaped",
                                    Some(pid),
                                ));
                            }
                            output.extend_from_slice(&buffer[..n]);
                            progressed = true;
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                            ) => {}
                        Err(e) => {
                            return Err(failure(
                                FailureKind::Other,
                                format!("Script-worker output failed: {e}"),
                                Some(pid),
                            ));
                        }
                    }
                }
                if let Some(status) = status {
                    if eof {
                        return Ok(Outcome {
                            status,
                            output,
                            pid,
                        });
                    }
                }
                if !progressed {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        })();
        // Explicit cleanup happens before any error is observable to the caller.
        // The Drop guard repeats it if unwinding interrupts the operation.
        if result.is_err() {
            owned.stop();
        }
        result
    }

    /// Called by the executable's early --script-worker branch, before App,
    /// fonts, display connection, or any browser/network handles are opened.
    pub fn worker_entry() -> ! {
        let probe = std::env::args().nth(2);
        if let Err(error) = install_isolation() {
            worker_error(
                &format!("Script-worker isolation unavailable: {error}"),
                SETUP_EXIT,
            );
        }
        if let Some(probe) = probe {
            match probe.strip_prefix("--probe=") {
                Some(mode) => run_probe(mode),
                None => worker_error("Unsupported script-worker argument", REQUEST_EXIT),
            }
        }
        let mut bytes = Vec::new();
        if let Err(error) = io::stdin()
            .take(INPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
        {
            worker_error(
                &format!("Cannot read script-worker request: {error}"),
                REQUEST_EXIT,
            );
        }
        if bytes.len() > INPUT_LIMIT {
            worker_error("Script-worker request exceeds 2 MiB", REQUEST_EXIT);
        }
        let request: Request = match serde_json::from_slice(&bytes) {
            Ok(request) => request,
            Err(error) => worker_error(
                &format!("Invalid script-worker request: {error}"),
                REQUEST_EXIT,
            ),
        };
        let reply = mg_deps::js_browser::execute(request);
        let bytes = match encode(&reply, OUTPUT_LIMIT) {
            Ok(bytes) => bytes,
            Err(_) => worker_error("Script-worker reply exceeds 4 MiB", REQUEST_EXIT),
        };
        if io::stdout().write_all(&bytes).is_err() {
            std::process::exit(REQUEST_EXIT);
        }
        std::process::exit(0)
    }
    fn worker_error(message: &str, code: i32) -> ! {
        let error = serde_json::json!({"worker_error":message});
        if let Ok(bytes) = encode(&error, 8192) {
            let _ = io::stdout().write_all(&bytes);
        }
        std::process::exit(code)
    }

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
    fn install_isolation() -> Result<(), String> {
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
    fn filter() -> Vec<libc::sock_filter> {
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

    fn expected_errno(result: libc::c_long, errno: i32, operation: &str) {
        if result != -1 || io::Error::last_os_error().raw_os_error() != Some(errno) {
            worker_error(
                &format!("Isolation probe failed: {operation}"),
                REQUEST_EXIT,
            );
        }
    }
    fn run_probe(mode: &str) -> ! {
        match mode {
            "deny" => {
                if std::env::vars_os().next().is_some() {
                    worker_error(
                        "Isolation probe failed: inherited environment",
                        REQUEST_EXIT,
                    );
                }
                // These are harmless, local capability checks in the owned
                // worker. Exact EPERM distinguishes seccomp from NOFILE/ENOENT.
                unsafe {
                    expected_errno(
                        libc::syscall(
                            libc::SYS_openat,
                            libc::AT_FDCWD,
                            c"/proc/self/status".as_ptr(),
                            libc::O_RDONLY,
                            0,
                        ),
                        libc::EPERM,
                        "file open",
                    );
                    expected_errno(
                        libc::syscall(libc::SYS_socket, libc::AF_INET, libc::SOCK_STREAM, 0),
                        libc::EPERM,
                        "socket creation",
                    );
                    // Deliberately invalid clone flags cannot create a child if
                    // a broken filter lets the syscall reach argument checking.
                    expected_errno(
                        libc::syscall(libc::SYS_clone, libc::CLONE_THREAD, 0, 0, 0, 0),
                        libc::EPERM,
                        "process creation",
                    );
                    expected_errno(
                        libc::syscall(
                            libc::SYS_execve,
                            c"/mgbrowser-nonexistent-isolation-probe".as_ptr(),
                            std::ptr::null::<*const u8>(),
                            std::ptr::null::<*const u8>(),
                        ),
                        libc::EPERM,
                        "exec",
                    );
                    expected_errno(
                        libc::syscall(libc::SYS_read, 3, std::ptr::null_mut::<u8>(), 0),
                        libc::EPERM,
                        "read outside stdin",
                    );
                    expected_errno(
                        libc::syscall(libc::SYS_write, 0, std::ptr::null::<u8>(), 0),
                        libc::EPERM,
                        "write outside output pipes",
                    );
                }
                let _ = io::stdout()
                    .write_all(b"denied:file,socket,process,exec,other-fd;environment:empty");
                std::process::exit(0)
            }
            "memory" => {
                // SAFETY: an anonymous request owns no existing mapping; only
                // an unexpected successful mapping needs to be unmapped.
                let allocation = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        512 * 1024 * 1024,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                        -1,
                        0,
                    )
                };
                if allocation != libc::MAP_FAILED {
                    unsafe {
                        libc::munmap(allocation, 512 * 1024 * 1024);
                    }
                    worker_error("Isolation probe failed: address-space limit", REQUEST_EXIT);
                }
                if io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
                    worker_error(
                        "Isolation probe did not observe RLIMIT_AS ENOMEM",
                        REQUEST_EXIT,
                    );
                }
                let _ = io::stdout().write_all(b"memory:ENOMEM");
                std::process::exit(0)
            }
            "cpu" => loop {
                std::hint::spin_loop();
            },
            "wall" => {
                std::thread::sleep(Duration::from_secs(10));
                worker_error("Parent failed to enforce wall deadline", REQUEST_EXIT)
            }
            "output" => {
                let bytes = [b'x'; 8192];
                loop {
                    if io::stdout().write_all(&bytes).is_err() {
                        std::process::exit(REQUEST_EXIT);
                    }
                }
            }
            _ => worker_error("Unknown isolation probe", REQUEST_EXIT),
        }
    }

    fn assert_reaped(pid: u32) -> Result<(), String> {
        let mut status = 0;
        // SAFETY: waitpid only examines the exact owned child PID; WNOHANG never
        // blocks or waits on an unrelated process. ECHILD proves no zombie is
        // retained by this parent after the runner returned.
        let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
        if result != -1 || io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD) {
            return Err(format!("Script worker {pid} was not already reaped"));
        }
        Ok(())
    }
    /// Run from a normal parent CLI, never inside a test harness's own process.
    /// Each check uses the same child launch, isolation and cleanup as execute.
    pub fn selftest() -> Result<(), String> {
        for (probe, expected) in [
            (
                "deny",
                &b"denied:file,socket,process,exec,other-fd;environment:empty"[..],
            ),
            ("memory", &b"memory:ENOMEM"[..]),
        ] {
            let outcome =
                run(&[], Some(probe), Instant::now() + WALL_LIMIT).map_err(|e| e.message)?;
            assert_reaped(outcome.pid)?;
            if !outcome.status.success() || outcome.output != expected {
                return Err(format!(
                    "Worker {probe} probe failed: {} {}",
                    outcome.status,
                    String::from_utf8_lossy(&outcome.output)
                ));
            }
            println!("SCRIPT_WORKER_CHECK {probe}: passed (child reaped)");
        }
        let cpu = run(&[], Some("cpu"), Instant::now() + WALL_LIMIT).map_err(|e| e.message)?;
        assert_reaped(cpu.pid)?;
        if !matches!(cpu.status.signal(), Some(libc::SIGKILL | libc::SIGXCPU)) {
            return Err(format!(
                "CPU limit did not terminate the worker: {}",
                cpu.status
            ));
        }
        println!("SCRIPT_WORKER_CHECK cpu: terminated by resource limit (child reaped)");
        for (probe, kind) in [
            ("wall", FailureKind::Deadline),
            ("output", FailureKind::OutputLimit),
        ] {
            match run(&[], Some(probe), Instant::now() + WALL_LIMIT) {
                Err(error) if error.kind == kind => {
                    assert_reaped(
                        error
                            .pid
                            .ok_or("Missing worker PID in bounded-run failure")?,
                    )?;
                    println!("SCRIPT_WORKER_CHECK {probe}: killed and reaped");
                }
                Err(error) => {
                    return Err(format!("Unexpected {probe} probe error: {}", error.message));
                }
                Ok(outcome) => {
                    return Err(format!(
                        "Worker {probe} probe unexpectedly exited: {}",
                        outcome.status
                    ));
                }
            }
        }
        let request = Request {
            url: "https://example.test/".into(),
            html: "<p>Script worker round trip</p>".into(),
        };
        let reply = execute(request)?;
        if reply.scripts_executed != 0
            || !reply.errors.is_empty()
            || !reply.html.contains("Script worker round trip")
        {
            return Err("Worker JSON/document round trip failed".into());
        }
        println!("SCRIPT_WORKER_CHECK protocol: bounded document round trip passed");
        let payload = "bounded payload ".repeat(8192);
        let bulk = execute(Request {
            url: "https://example.test/large-owned-fixture".into(),
            html: format!("<p>{payload}</p>"),
        })?;
        if bulk.scripts_executed != 0 || !bulk.errors.is_empty() || !bulk.html.contains(&payload) {
            return Err("Worker multi-buffer document transfer failed".into());
        }
        println!("SCRIPT_WORKER_CHECK bulk: multi-buffer document round trip passed");
        let oversized = Request {
            url: "https://example.test/".into(),
            html: "x".repeat(INPUT_LIMIT),
        };
        match execute(oversized) {
            Err(error) if error.starts_with("Script-worker request exceeds 2 MiB") => {}
            _ => return Err("Oversized worker input was not rejected before launch".into()),
        }
        println!("SCRIPT_WORKER_CHECK input: oversized request rejected before launch");
        println!("SCRIPT_WORKER_SELFTEST_OK");
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn serializer_rejects_expansion_without_growing_past_limit() {
            let mut output = LimitedBuffer {
                bytes: Vec::new(),
                limit: 16,
            };
            assert!(serde_json::to_writer(&mut output, &"\n".repeat(32)).is_err());
            assert!(output.bytes.len() <= 16);
        }
        #[test]
        fn syscall_filter_has_bounded_program_and_default_deny() {
            let instructions = filter();
            assert!(instructions.len() < 256);
            assert_eq!(
                instructions.last().unwrap().k,
                libc::SECCOMP_RET_ERRNO | libc::EPERM as u32
            );
        }

        // Model only the four classic-BPF instruction forms emitted above.
        // Actual kernel installation and denial are separate subprocess tests.
        fn decision(arch: u32, syscall: libc::c_long, args: [u64; 6]) -> u32 {
            let mut data = [0u32; 16];
            data[0] = syscall as u32;
            data[1] = arch;
            for (index, argument) in args.iter().enumerate() {
                data[4 + index * 2] = *argument as u32;
                data[5 + index * 2] = (*argument >> 32) as u32;
            }
            let program = filter();
            let mut accumulator = 0;
            let mut pc = 0;
            for _ in 0..256 {
                let instruction = &program[pc];
                match instruction.code as u32 {
                    code if code == libc::BPF_LD | libc::BPF_W | libc::BPF_ABS => {
                        accumulator = data[instruction.k as usize / 4];
                        pc += 1;
                    }
                    code if code == libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K => {
                        pc += 1 + if accumulator == instruction.k {
                            instruction.jt as usize
                        } else {
                            instruction.jf as usize
                        };
                    }
                    code if code == libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K => {
                        pc += 1 + if accumulator & instruction.k != 0 {
                            instruction.jt as usize
                        } else {
                            instruction.jf as usize
                        };
                    }
                    code if code == libc::BPF_RET | libc::BPF_K => return instruction.k,
                    _ => panic!("Unexpected worker filter instruction"),
                }
            }
            panic!("Worker filter did not return");
        }

        #[test]
        fn syscall_filter_enforces_arch_descriptors_and_nonexecutable_memory() {
            let arch = 0xc000_003e;
            let denied = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;
            let check = |syscall, args| decision(arch, syscall, args);
            assert_eq!(check(libc::SYS_read, [0; 6]), libc::SECCOMP_RET_ALLOW);
            for fd in [1, 2] {
                assert_eq!(
                    check(libc::SYS_write, [fd, 0, 0, 0, 0, 0]),
                    libc::SECCOMP_RET_ALLOW
                );
            }
            for (syscall, fd) in [
                (libc::SYS_read, 1),
                (libc::SYS_read, 3),
                (libc::SYS_write, 0),
                (libc::SYS_write, 3),
                (libc::SYS_write, (1u64 << 32) | 1),
            ] {
                assert_eq!(check(syscall, [fd, 0, 0, 0, 0, 0]), denied);
            }
            for fd in [u32::MAX as u64, u64::MAX] {
                let mut args = [
                    0,
                    4096,
                    (libc::PROT_READ | libc::PROT_WRITE) as u64,
                    (libc::MAP_PRIVATE | libc::MAP_ANONYMOUS) as u64,
                    fd,
                    0,
                ];
                assert_eq!(check(libc::SYS_mmap, args), libc::SECCOMP_RET_ALLOW);
                args[2] |= libc::PROT_EXEC as u64;
                assert_eq!(check(libc::SYS_mmap, args), denied);
                args[2] = libc::PROT_READ as u64;
                args[3] = libc::MAP_PRIVATE as u64;
                assert_eq!(check(libc::SYS_mmap, args), denied);
            }
            assert_eq!(
                check(
                    libc::SYS_mprotect,
                    [0, 4096, libc::PROT_EXEC as u64, 0, 0, 0]
                ),
                denied
            );
            for syscall in [
                libc::SYS_openat,
                libc::SYS_socket,
                libc::SYS_clone,
                libc::SYS_execve,
                libc::SYS_prctl,
                libc::SYS_dup,
                libc::SYS_read | 0x4000_0000, // x32 ABI uses this extra bit.
            ] {
                assert_eq!(check(syscall, [0; 6]), denied);
            }
            assert_eq!(
                decision(0x4000_0003, libc::SYS_read, [0; 6]),
                libc::SECCOMP_RET_KILL_PROCESS
            );
        }
    }
}
