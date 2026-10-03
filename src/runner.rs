use crate::{config::PreparedBlock, RuntimeEvent};
use std::{
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const STDERR_LIMIT: usize = 8 * 1024;
const POLL_TIMEOUT_MS: i32 = 20;
const TERMINATION_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub(crate) enum RunResult {
    Completed {
        index: usize,
        rendered: String,
        diagnostic: Option<String>,
    },
    Failed {
        index: usize,
        diagnostic: String,
    },
}

impl RunResult {
    pub fn index(&self) -> usize {
        match self {
            Self::Completed { index, .. } | Self::Failed { index, .. } => *index,
        }
    }
}

pub(crate) struct RunnerPool {
    slots: Vec<Option<JoinHandle<()>>>,
    process_groups: Arc<Mutex<Vec<Option<i32>>>>,
    events: Sender<RuntimeEvent>,
    shutdown: Arc<AtomicBool>,
}

impl RunnerPool {
    pub fn new(
        block_count: usize,
        events: Sender<RuntimeEvent>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            slots: (0..block_count).map(|_| None).collect(),
            process_groups: Arc::new(Mutex::new(vec![None; block_count])),
            events,
            shutdown,
        }
    }

    pub fn start(&mut self, index: usize, block: PreparedBlock) -> crate::types::Result<()> {
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(crate::error::AtomBlocksError::Runtime(format!(
                "invalid block index {index}"
            )));
        };
        if slot.is_some() {
            return Err(crate::error::AtomBlocksError::Runtime(format!(
                "block {index} is already running"
            )));
        }

        let events = self.events.clone();
        let groups = self.process_groups.clone();
        let shutdown = self.shutdown.clone();
        let handle = thread::Builder::new()
            .name(format!("atomblocks-runner-{index}"))
            .spawn(move || {
                if let Some(result) = run_block(index, block, &groups, &shutdown) {
                    let _ = events.send(RuntimeEvent::Runner(result));
                }
            })?;
        *slot = Some(handle);
        Ok(())
    }

    pub fn finish(&mut self, index: usize) -> crate::types::Result<()> {
        let Some(slot) = self.slots.get_mut(index) else {
            return Err(crate::error::AtomBlocksError::Runtime(format!(
                "invalid block index {index}"
            )));
        };
        if let Some(handle) = slot.take() {
            handle.join().map_err(|_| {
                crate::error::AtomBlocksError::Runtime(format!(
                    "block {index} runner thread panicked"
                ))
            })?;
        }
        Ok(())
    }

    pub fn shutdown_and_join(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let groups = self
            .process_groups
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for pgid in groups.iter().flatten() {
            signal_process_group(*pgid, libc::SIGTERM);
        }
        drop(groups);

        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some(handle) = slot.take() {
                if handle.join().is_err() {
                    log::error!("block {index} runner thread panicked during shutdown");
                }
            }
        }
    }
}

fn run_block(
    index: usize,
    block: PreparedBlock,
    process_groups: &Arc<Mutex<Vec<Option<i32>>>>,
    shutdown: &Arc<AtomicBool>,
) -> Option<RunResult> {
    if shutdown.load(Ordering::Acquire) {
        return None;
    }

    log::debug!("Run block {index}: {}", block.block.execute);
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(&block.block.execute)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Some(RunResult::Failed {
                index,
                diagnostic: format!("block {index}: failed to spawn command: {error}"),
            });
        }
    };

    let pgid = child.id() as i32;
    {
        let mut groups = process_groups
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        groups[index] = Some(pgid);
    }

    let result = collect_child(index, &mut child, pgid, &block, shutdown);

    {
        let mut groups = process_groups
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        groups[index] = None;
    }

    if shutdown.load(Ordering::Acquire) {
        None
    } else {
        Some(result)
    }
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    overflow: bool,
}

fn collect_child(
    index: usize,
    child: &mut Child,
    pgid: i32,
    block: &PreparedBlock,
    shutdown: &Arc<AtomicBool>,
) -> RunResult {
    let Some(mut stdout) = child.stdout.take() else {
        cleanup_child(child, pgid);
        return failed(index, "stdout pipe was not available");
    };
    let Some(mut stderr) = child.stderr.take() else {
        cleanup_child(child, pgid);
        return failed(index, "stderr pipe was not available");
    };

    if let Err(error) = set_nonblocking(&stdout).and_then(|_| set_nonblocking(&stderr)) {
        cleanup_child(child, pgid);
        return failed(index, format!("failed to configure command pipes: {error}"));
    }

    let started = Instant::now();
    let mut stdout_capture = Capture::default();
    let mut stderr_capture = Capture::default();
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut status = None;
    let mut termination_started = None;
    let mut killed = false;
    let mut timed_out = false;
    let mut stale_pipes = false;

    loop {
        if stdout_open {
            match drain_pipe(&mut stdout, &mut stdout_capture, block.output_limit) {
                Ok(open) => stdout_open = open,
                Err(error) => {
                    cleanup_child(child, pgid);
                    return failed(index, format!("failed to read stdout: {error}"));
                }
            }
        }
        if stderr_open {
            match drain_pipe(&mut stderr, &mut stderr_capture, STDERR_LIMIT) {
                Ok(open) => stderr_open = open,
                Err(error) => {
                    cleanup_child(child, pgid);
                    return failed(index, format!("failed to read stderr: {error}"));
                }
            }
        }

        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    signal_process_group(pgid, libc::SIGTERM);
                    termination_started.get_or_insert_with(Instant::now);
                }
                Ok(None) => {}
                Err(error) => {
                    cleanup_child(child, pgid);
                    return failed(index, format!("failed to wait for command: {error}"));
                }
            }
        }

        if status.is_none() && shutdown.load(Ordering::Acquire) && termination_started.is_none() {
            signal_process_group(pgid, libc::SIGTERM);
            termination_started = Some(Instant::now());
        }

        if status.is_none() && !timed_out {
            if let Some(timeout) = block.timeout {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    signal_process_group(pgid, libc::SIGTERM);
                    termination_started.get_or_insert_with(Instant::now);
                }
            }
        }

        if let Some(started) = termination_started {
            if !killed && started.elapsed() >= TERMINATION_GRACE {
                signal_process_group(pgid, libc::SIGKILL);
                killed = true;
            }
            if killed
                && started.elapsed() >= TERMINATION_GRACE + TERMINATION_GRACE
                && (stdout_open || stderr_open)
            {
                stale_pipes = true;
                break;
            }
        }

        if status.is_some() && !stdout_open && !stderr_open {
            break;
        }

        poll_pipes(&stdout, stdout_open, &stderr, stderr_open, POLL_TIMEOUT_MS);
    }

    if status.is_none() {
        status = child.try_wait().ok().flatten();
    }
    if status.is_none() {
        signal_process_group(pgid, libc::SIGKILL);
        status = child.wait().ok();
    }

    if shutdown.load(Ordering::Acquire) {
        return failed(index, "command cancelled during shutdown");
    }
    if timed_out {
        return failed(
            index,
            format!(
                "command timed out after {:.3}s",
                block.timeout.unwrap_or_default().as_secs_f64()
            ),
        );
    }
    if stale_pipes {
        return failed(
            index,
            "command descendants kept stdout/stderr open after process-group termination",
        );
    }
    if stdout_capture.overflow {
        return failed(
            index,
            format!(
                "stdout exceeded output_limit ({} bytes)",
                block.output_limit
            ),
        );
    }

    let Some(status) = status else {
        return failed(index, "command status was unavailable");
    };
    let stderr = captured_text(&stderr_capture);
    let diagnostic = if status.success() {
        None
    } else if stderr.is_empty() {
        Some(format!("block {index}: command exited with {status}"))
    } else {
        Some(format!(
            "block {index}: command exited with {status}; stderr: {stderr}"
        ))
    };

    RunResult::Completed {
        index,
        rendered: block
            .block
            .print(String::from_utf8_lossy(&stdout_capture.bytes).to_string()),
        diagnostic,
    }
}

fn failed(index: usize, message: impl Into<String>) -> RunResult {
    RunResult::Failed {
        index,
        diagnostic: format!("block {index}: {}", message.into()),
    }
}

fn captured_text(capture: &Capture) -> String {
    let mut text = String::from_utf8_lossy(&capture.bytes).trim().to_owned();
    if capture.overflow {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str("[truncated]");
    }
    text
}

fn drain_pipe<R: Read>(reader: &mut R, capture: &mut Capture, limit: usize) -> io::Result<bool> {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(false),
            Ok(read) => {
                let available = limit.saturating_sub(capture.bytes.len());
                let keep = available.min(read);
                capture.bytes.extend_from_slice(&buffer[..keep]);
                if keep < read {
                    capture.overflow = true;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn set_nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    let fd = fd.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn poll_pipes(
    stdout: &ChildStdout,
    stdout_open: bool,
    stderr: &ChildStderr,
    stderr_open: bool,
    timeout_ms: i32,
) {
    let mut pollfds = Vec::with_capacity(2);
    if stdout_open {
        pollfds.push(libc::pollfd {
            fd: stdout.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        });
    }
    if stderr_open {
        pollfds.push(libc::pollfd {
            fd: stderr.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        });
    }

    if pollfds.is_empty() {
        thread::sleep(Duration::from_millis(timeout_ms as u64));
        return;
    }

    unsafe {
        libc::poll(
            pollfds.as_mut_ptr(),
            pollfds.len() as libc::nfds_t,
            timeout_ms,
        );
    }
}

fn cleanup_child(child: &mut Child, pgid: i32) {
    signal_process_group(pgid, libc::SIGTERM);
    let deadline = Instant::now() + TERMINATION_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => break,
        }
    }
    signal_process_group(pgid, libc::SIGKILL);
    let _ = child.wait();
}

fn signal_process_group(pgid: i32, signal: i32) {
    if pgid <= 0 {
        return;
    }
    let result = unsafe { libc::kill(-pgid, signal) };
    if result == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            log::warn!("failed to signal process group {pgid}: {error}");
        }
    }
}
