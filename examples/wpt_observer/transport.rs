//! Host-only acceptance transport. No page JS, network, display or renewed clocks.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::Path,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, String>;
pub const INPUT: usize = 2 * 1024 * 1024;
pub const OUTPUT: usize = 4 * 1024 * 1024;
pub const WIRE: usize = 32 * 1024 * 1024;
pub const TRANSACTIONS: u64 = 64;
const STDERR: usize = 4096;
static CANCELED: AtomicBool = AtomicBool::new(false);

extern "C" fn cancel(_: libc::c_int) {
    CANCELED.store(true, Ordering::Relaxed);
}

pub struct Signals([libc::sighandler_t; 2]);
impl Signals {
    pub fn install() -> Result<Self> {
        // This single-threaded Linux-only tool uses a lock-free scalar handler;
        // it cannot panic between spawn returning and ownership construction.
        let int = unsafe { libc::signal(libc::SIGINT, cancel as libc::sighandler_t) };
        if int == libc::SIG_ERR {
            return Err("Cannot install INT handler".into());
        }
        let term = unsafe { libc::signal(libc::SIGTERM, cancel as libc::sighandler_t) };
        if term == libc::SIG_ERR {
            unsafe {
                libc::signal(libc::SIGINT, int);
            }
            return Err("Cannot install TERM handler".into());
        }
        Ok(Self([int, term]))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.0[0]);
            libc::signal(libc::SIGTERM, self.0[1]);
        }
    }
}

pub struct Budget {
    pub started: Instant,
    pub used: Duration,
    pub cutoff: Instant,
    pub whole: Instant,
    accounted_through: Instant,
}
impl Budget {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            used: Duration::ZERO,
            cutoff: started + Duration::from_secs(23),
            whole: started + Duration::from_secs(25),
            accounted_through: started,
        }
    }
    fn admission_at(&self, now: Instant) -> Result<()> {
        if CANCELED.load(Ordering::Relaxed) {
            return Err("Parent canceled the owned control".into());
        }
        if now >= self.cutoff {
            return Err("Observation admission stopped at 23 seconds".into());
        }
        Ok(())
    }
    fn remaining(&self) -> Result<Duration> {
        Duration::from_secs(2)
            .checked_sub(self.used)
            .filter(|v| !v.is_zero())
            .ok_or_else(|| "Cumulative active allowance exhausted".into())
    }
    fn validate_at(&self, now: Instant) -> Result<()> {
        if CANCELED.load(Ordering::Relaxed)
            || now >= self.whole
            || self.used >= Duration::from_secs(2)
        {
            return Err("Final cancellation/absolute/cumulative-active guard failed".into());
        }
        Ok(())
    }
    fn account_until(&mut self, now: Instant) -> Result<()> {
        let elapsed = now
            .checked_duration_since(self.accounted_through)
            .ok_or("Monotonic accounting cursor moved backward")?;
        self.used = self.used.saturating_add(elapsed);
        self.accounted_through = now;
        self.validate_at(now)
    }
    /// No admission or completion acknowledgment: just consume actual parent
    /// bookkeeping/idle validation from the SAME active allowance, even at23s.
    pub fn account_nonadmitting(&mut self) -> Result<()> {
        self.account_until(Instant::now())
    }
    fn deadline_at(&mut self, now: Instant) -> Result<Instant> {
        self.account_until(now)?;
        self.admission_at(now)?;
        Ok((now + self.remaining()?).min(self.cutoff))
    }
    pub fn deadline(&mut self) -> Result<Instant> {
        self.deadline_at(Instant::now())
    }
    /// Exactly one completion charge, after full decode/validate/project/stage.
    pub fn charge(&mut self) -> Result<()> {
        self.account_nonadmitting()?;
        self.admission_at(Instant::now())
    }
    fn exclude_interval(&mut self, start: Instant, end: Instant) -> Result<()> {
        // The small interval before the actual sleep/cleanup call is active.
        // Only that call's measured start..end is excluded; validation is not.
        if end < start {
            return Err("Invalid excluded clock interval".into());
        }
        let active = self.account_until(start);
        self.accounted_through = end;
        active.and(self.validate_at(end))
    }
    pub fn sleep_idle(&mut self, duration: Duration) -> Result<()> {
        self.account_nonadmitting()?;
        let start = Instant::now();
        std::thread::sleep(duration);
        self.exclude_interval(start, Instant::now())
    }
    pub fn exclude_owned_cleanup(&mut self, start: Instant, end: Instant) -> Result<()> {
        let result = self.exclude_interval(start, end);
        if end.duration_since(start) > Duration::from_secs(2) {
            return Err("Owned cleanup exceeded its unchanged two-second reserve".into());
        }
        result
    }
    pub fn final_guard(&mut self) -> Result<()> {
        self.account_nonadmitting()
    }
}

fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}
fn retry(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

pub fn regular(path: &Path, limit: usize) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    let info = file.metadata().map_err(|e| e.to_string())?;
    if !info.is_file() || info.len() > limit as u64 {
        return Err("Unsafe or oversized input file".into());
    }
    Ok(file)
}

pub struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    errors: Option<ChildStderr>,
    pub wire: usize,
    pub stderr: Vec<u8>,
    pub cleanup_confirmed: bool,
    pub exit_code: Option<i32>,
    reaped: bool,
    cleanup_deadline: Instant,
}
pub struct StartFailure {
    pub message: String,
    pub pid: Option<u32>,
    pub cleanup_confirmed: bool,
}
impl Worker {
    pub fn start(binary: &Path, whole: Instant) -> std::result::Result<Self, StartFailure> {
        let child = Command::new(binary)
            .arg("--script-session")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|e| StartFailure {
                message: e.to_string(),
                pid: None,
                cleanup_confirmed: true,
            })?;
        // No allocating/fallible owner wrapper between spawn and this RAII guard.
        let mut owner = Self {
            child,
            input: None,
            output: None,
            errors: None,
            wire: 0,
            stderr: Vec::new(),
            cleanup_confirmed: false,
            exit_code: None,
            reaped: false,
            cleanup_deadline: whole,
        };
        let setup: Result<()> = (|| {
            owner.input = Some(owner.child.stdin.take().ok_or("Missing stdin")?);
            owner.output = Some(owner.child.stdout.take().ok_or("Missing stdout")?);
            owner.errors = Some(owner.child.stderr.take().ok_or("Missing stderr")?);
            for fd in [
                owner.input.as_ref().unwrap().as_raw_fd(),
                owner.output.as_ref().unwrap().as_raw_fd(),
                owner.errors.as_ref().unwrap().as_raw_fd(),
            ] {
                nonblocking(fd)?;
            }
            Ok(())
        })();
        if let Err(message) = setup {
            let _ = owner.stop();
            let cleanup_confirmed = owner.cleanup_confirmed;
            return Err(StartFailure {
                message,
                pid: Some(owner.pid()),
                cleanup_confirmed,
            });
        }
        Ok(owner)
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    fn exited(&self) -> Result<bool> {
        // Observe without reaping; the owned leader PID cannot be reused before
        // killpg. Never call Child::try_wait while this guard is live.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(unsafe { info.si_pid() } != 0)
    }
    fn add_wire(&mut self, bytes: usize) -> Result<()> {
        self.wire = self
            .wire
            .checked_add(bytes)
            .filter(|n| *n <= WIRE)
            .ok_or("Combined cumulative wire exceeds 32 MiB")?;
        Ok(())
    }
    pub fn drain_stderr(&mut self) -> Result<()> {
        let mut buffer = [0; 1024];
        loop {
            match self
                .errors
                .as_mut()
                .ok_or("Missing stderr")?
                .read(&mut buffer)
            {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    if n > STDERR.saturating_sub(self.stderr.len()) {
                        return Err("Worker stderr exceeds 4 KiB".into());
                    }
                    self.stderr.extend_from_slice(&buffer[..n]);
                }
                Err(e) if retry(&e) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    fn guard(&mut self, deadline: Instant) -> Result<()> {
        if CANCELED.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err("Active deadline/cancellation stopped duplex transaction".into());
        }
        self.drain_stderr()?;
        Ok(())
    }
    pub fn tail(&mut self, terminal: bool) -> Result<()> {
        let mut byte = [0];
        match self
            .output
            .as_mut()
            .ok_or("Missing stdout")?
            .read(&mut byte)
        {
            Ok(0) if terminal => Ok(()),
            Ok(0) => Err("Ready worker closed stdout unexpectedly".into()),
            Ok(n) => {
                self.add_wire(n)?;
                Err("Extra data after reply frame".into())
            }
            Err(e) if retry(&e) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn idle(&mut self) -> Result<()> {
        self.drain_stderr()?;
        if CANCELED.load(Ordering::Relaxed) || self.exited()? {
            return Err("Worker canceled/exited while idle".into());
        }
        let mut byte = [0];
        match self
            .output
            .as_mut()
            .ok_or("Missing stdout")?
            .read(&mut byte)
        {
            Ok(0) => Err("Ready worker closed stdout while idle".into()),
            Ok(n) => {
                self.add_wire(n)?;
                Err("Unsolicited worker output".into())
            }
            Err(e) if retry(&e) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn exchange(
        &mut self,
        payload: &[u8],
        limit: usize,
        deadline: Instant,
        sent_file: &mut File,
        received_file: &mut File,
    ) -> Result<Vec<u8>> {
        if payload.is_empty() || payload.len() > limit || payload.len() > INPUT {
            return Err("Request exceeds its existing frame admission".into());
        }
        if payload.len() + 4 > WIRE.saturating_sub(self.wire) {
            return Err("Wire cannot admit request".into());
        }
        let header = (payload.len() as u32).to_be_bytes();
        let (mut sent_header, mut sent, mut header_read) = (0, 0, 0);
        let mut received_header = [0; 4];
        let mut output = Vec::new();
        let mut expected = None;
        let mut buffer = [0; 16384];
        loop {
            self.guard(deadline)?;
            let mut progressed = false;
            let input = if sent_header < 4 {
                &header[sent_header..]
            } else {
                &payload[sent..(sent + buffer.len()).min(payload.len())]
            };
            if !input.is_empty() {
                match self.input.as_mut().ok_or("Closed stdin")?.write(input) {
                    Ok(0) => return Err("Worker stdin closed".into()),
                    Ok(n) => {
                        sent_file
                            .write_all(&input[..n])
                            .map_err(|e| e.to_string())?;
                        if sent_header < 4 {
                            sent_header += n;
                        } else {
                            sent += n;
                        }
                        self.add_wire(n)?;
                        progressed = true;
                    }
                    Err(e) if retry(&e) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            let read = if header_read < 4 {
                self.output
                    .as_mut()
                    .unwrap()
                    .read(&mut received_header[header_read..])
            } else {
                let count = expected
                    .unwrap_or(0usize)
                    .saturating_sub(output.len())
                    .min(buffer.len());
                self.output.as_mut().unwrap().read(&mut buffer[..count])
            };
            match read {
                Ok(0) => return Err("Truncated reply frame".into()),
                Ok(n) => {
                    self.add_wire(n)?;
                    progressed = true;
                    if header_read < 4 {
                        received_file
                            .write_all(&received_header[header_read..header_read + n])
                            .map_err(|e| e.to_string())?;
                        header_read += n;
                        if header_read == 4 {
                            let size = u32::from_be_bytes(received_header) as usize;
                            if size == 0 || size > OUTPUT || size > WIRE.saturating_sub(self.wire) {
                                return Err(
                                    "Invalid/over-limit response length before allocation".into()
                                );
                            }
                            output.try_reserve_exact(size).map_err(|e| e.to_string())?;
                            expected = Some(size);
                        }
                    } else {
                        received_file
                            .write_all(&buffer[..n])
                            .map_err(|e| e.to_string())?;
                        output.extend_from_slice(&buffer[..n]);
                    }
                }
                Err(e) if retry(&e) => {}
                Err(e) => return Err(e.to_string()),
            }
            if expected == Some(output.len()) {
                if sent_header != 4 || sent != payload.len() {
                    return Err("Reply preceded complete request".into());
                }
                return Ok(output);
            }
            if !progressed {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.cleanup_confirmed {
            return Ok(());
        }
        self.cleanup_deadline = self
            .cleanup_deadline
            .min(Instant::now() + Duration::from_secs(2));
        self.input.take();
        if !self.reaped {
            let killed = unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL) };
            if killed < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error.to_string());
                }
            }
            while !self.exited()? {
                if Instant::now() >= self.cleanup_deadline {
                    return Err("Owned group cleanup unconfirmed by deadline".into());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            // The first and only reap follows the exact group's kill.
            let status = self.child.wait().map_err(|e| e.to_string())?;
            self.exit_code = status.code();
            self.reaped = true;
        }
        loop {
            // Read-only after reap: never signal a possibly reused group ID.
            let result = unsafe { libc::kill(-(self.child.id() as libc::pid_t), 0) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    break;
                }
                return Err(format!("Owned group absence unconfirmed: {error}"));
            }
            if Instant::now() >= self.cleanup_deadline {
                return Err("Owned group still exists at cleanup deadline".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.cleanup_confirmed = true;
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_phases_share_one_active_allowance_without_refund() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        budget
            .account_until(start + Duration::from_millis(1900))
            .unwrap();
        assert!(
            budget
                .account_until(start + Duration::from_millis(2100))
                .is_err()
        );
        assert!(budget.used >= Duration::from_secs(2));
        assert!(budget.remaining().is_err());
    }

    #[test]
    fn validated_idle_does_not_renew_already_charged_work() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        let before_idle = start + Duration::from_millis(1500);
        let after_idle = before_idle + Duration::from_secs(10);
        budget.account_until(before_idle).unwrap();
        budget.exclude_interval(before_idle, after_idle).unwrap();
        let remaining = budget.deadline_at(after_idle).unwrap() - after_idle;
        assert_eq!(remaining, Duration::from_millis(500));
        budget
            .account_until(after_idle + Duration::from_millis(100))
            .unwrap();
        assert!(budget.used >= Duration::from_millis(1500));
    }

    #[test]
    fn execution_cutoff_preserves_cleanup_window_not_new_admission() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        let cutoff = start + Duration::from_secs(23);
        budget.exclude_interval(start, cutoff).unwrap();
        assert!(budget.admission_at(cutoff).is_err());
        assert!(budget.deadline_at(cutoff).is_err());
        assert!(budget.account_until(cutoff).is_ok());
        assert!(budget.admission_at(cutoff).is_err());
        assert!(budget.whole > cutoff);
    }

    #[test]
    fn only_sleep_interval_is_free_not_idle_checks_or_bookkeeping() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        budget
            .account_until(start + Duration::from_millis(10))
            .unwrap();
        // Ten more milliseconds of controls precede an 80ms parent sleep.
        budget
            .exclude_interval(
                start + Duration::from_millis(20),
                start + Duration::from_millis(100),
            )
            .unwrap();
        budget
            .account_until(start + Duration::from_millis(110))
            .unwrap();
        assert_eq!(budget.used, Duration::from_millis(30));
        assert_eq!(budget.remaining().unwrap(), Duration::from_millis(1970));
    }
}
