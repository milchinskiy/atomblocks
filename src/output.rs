use crate::{error::AtomBlocksError, OutputMode, RuntimeEvent};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, PropMode},
    wrapper::ConnectionExt as _,
};

struct Pending {
    latest: Option<String>,
    stopped: bool,
}

struct Shared {
    pending: Mutex<Pending>,
    changed: Condvar,
    shutdown: Arc<AtomicBool>,
}

pub(crate) struct OutputController {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl OutputController {
    pub fn start(
        mode: OutputMode,
        events: Sender<RuntimeEvent>,
        shutdown: Arc<AtomicBool>,
    ) -> crate::types::Result<Self> {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending {
                latest: None,
                stopped: false,
            }),
            changed: Condvar::new(),
            shutdown,
        });
        let worker = shared.clone();
        let thread = thread::Builder::new()
            .name("atomblocks-output".into())
            .spawn(move || {
                let result = match mode {
                    OutputMode::Stdout => run_stdout(&worker),
                    OutputMode::X11 => run_x11(&worker),
                };
                if let Err(error) = result {
                    let _ = events.send(RuntimeEvent::Fatal(error));
                }
            })?;

        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn publish(&self, value: String) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.stopped {
            return;
        }
        pending.latest = Some(value);
        self.shared.changed.notify_one();
    }

    pub fn shutdown(&mut self) {
        {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.stopped = true;
            pending.latest = None;
        }
        self.shared.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("output thread panicked during shutdown");
            }
        }
    }
}

fn next_value(shared: &Shared) -> Option<String> {
    let mut pending = shared
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        if pending.stopped || shared.shutdown.load(Ordering::Acquire) {
            return None;
        }
        if let Some(value) = pending.latest.take() {
            return Some(value);
        }
        pending = shared
            .changed
            .wait(pending)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

fn run_stdout(shared: &Shared) -> crate::types::Result<()> {
    let _flags = NonblockingFd::new(libc::STDOUT_FILENO)?;
    while let Some(value) = next_value(shared) {
        let record = format_stdout_record(&value);
        write_interruptible(libc::STDOUT_FILENO, record.as_bytes(), shared)?;
    }
    Ok(())
}

fn run_x11(shared: &Shared) -> crate::types::Result<()> {
    let (connection, root, _) = crate::x11::x11_connect()?;
    while let Some(value) = next_value(shared) {
        log::info!("Updating WM_NAME property...");
        connection
            .change_property8(
                PropMode::REPLACE,
                root,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                value.as_bytes(),
            )?
            .check()?;
        connection.flush()?;
    }
    Ok(())
}

fn format_stdout_record(bar: &str) -> String {
    let mut record = bar.replace(['\r', '\n'], "");
    record.push('\n');
    record
}

fn write_interruptible(fd: i32, bytes: &[u8], shared: &Shared) -> crate::types::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        if shared.shutdown.load(Ordering::Acquire) {
            return Ok(());
        }
        let stopped = shared
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stopped;
        if stopped {
            return Ok(());
        }

        let result =
            unsafe { libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written) };
        if result > 0 {
            written += result as usize;
            continue;
        }
        if result == 0 {
            return Err(AtomBlocksError::IOError(io::Error::new(
                io::ErrorKind::WriteZero,
                "failed to write bar update",
            )));
        }

        let error = io::Error::last_os_error();
        match error.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => wait_writable(fd),
            _ => return Err(AtomBlocksError::IOError(error)),
        }
    }
    Ok(())
}

fn wait_writable(fd: i32) {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLOUT | libc::POLLERR | libc::POLLHUP,
        revents: 0,
    };
    unsafe {
        libc::poll(&mut pollfd, 1, 100);
    }
}

struct NonblockingFd {
    fd: i32,
    flags: i32,
}

impl NonblockingFd {
    fn new(fd: i32) -> io::Result<Self> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd, flags })
    }
}

impl Drop for NonblockingFd {
    fn drop(&mut self) {
        unsafe {
            libc::fcntl(self.fd, libc::F_SETFL, self.flags);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdout_record_is_one_flushed_line_contract() {
        assert_eq!(
            format_stdout_record("[α\r\n] |  β \nnext"),
            "[α] |  β next\n"
        );
        assert_eq!(format_stdout_record(""), "\n");
    }
}
