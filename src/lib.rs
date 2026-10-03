use config::{Config, PreparedConfig};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{self, RecvTimeoutError},
        Arc,
    },
    time::{Duration, Instant},
};
use x11rb::{
    protocol::xproto::{AtomEnum, PropMode},
    wrapper::ConnectionExt as _,
};

pub mod atoms;
pub mod cli;
pub mod config;
pub mod error;
mod output;
mod runner;
mod scheduler;
pub mod types;
mod x11;

const SIGNAL_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct HitMan {
    xconn: x11rb::rust_connection::RustConnection,
    root: u32,
    atoms: atoms::AtomBlocksAtoms,
}

impl HitMan {
    pub fn new() -> types::Result<Self> {
        let (xconn, root, atoms) = x11::x11_connect()?;
        Ok(Self { xconn, root, atoms })
    }

    pub fn hit_block(&self, id: u32) -> types::Result<()> {
        self.xconn
            .change_property32(
                PropMode::APPEND,
                self.root,
                self.atoms._ATOMBLOCKS_HIT_QUEUE,
                AtomEnum::INTEGER,
                &[id],
            )?
            .check()?;
        Ok(())
    }
}

/// Destination for bar updates. Both modes retain X11-based manual updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    X11,
    Stdout,
}

pub struct AtomBlocks {
    config: PreparedConfig,
    output: OutputMode,
    cells: Vec<String>,
    x11: x11::Backend,
}

#[derive(Debug)]
pub(crate) enum RuntimeEvent {
    Runner(runner::RunResult),
    HitsReady,
    Fatal(error::AtomBlocksError),
}

impl AtomBlocks {
    pub fn new(config: Config) -> types::Result<Self> {
        Self::new_with_output(config, OutputMode::X11)
    }

    /// Select the bar output destination; an X11 connection is still required.
    pub fn new_with_output(config: Config, output: OutputMode) -> types::Result<Self> {
        // Validate all runtime values before opening X11 or starting any worker.
        let config = config.prepare()?;
        let x11 = x11::Backend::connect()?;
        let cells = vec![String::new(); config.blocks.len()];
        log::trace!("Allocated {} cells", cells.len());
        Ok(Self {
            config,
            output,
            cells,
            x11,
        })
    }

    pub fn run(&mut self) -> types::Result<()> {
        let _signals = SignalGuard::install()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let (events, receiver) = mpsc::channel::<RuntimeEvent>();
        let hits = Arc::new(x11::HitInbox::new(self.config.blocks.len()));

        self.x11
            .start(hits.clone(), events.clone(), shutdown.clone())?;
        let mut output =
            match output::OutputController::start(self.output, events.clone(), shutdown.clone()) {
                Ok(output) => output,
                Err(error) => {
                    shutdown.store(true, Ordering::Release);
                    self.x11.shutdown();
                    return Err(error);
                }
            };
        let mut runners =
            runner::RunnerPool::new(self.config.blocks.len(), events.clone(), shutdown.clone());
        drop(events);

        let now = Instant::now();
        let mut scheduler =
            scheduler::Scheduler::new(self.config.blocks.iter().map(|block| block.interval), now);
        let mut last_diagnostics = vec![None::<String>; self.config.blocks.len()];

        log::info!("Ready to receive events");
        let result = self.run_loop(
            &receiver,
            &hits,
            &mut scheduler,
            &mut runners,
            &output,
            &mut last_diagnostics,
        );

        shutdown.store(true, Ordering::Release);
        self.x11.shutdown();
        runners.shutdown_and_join();
        output.shutdown();
        result
    }

    fn run_loop(
        &mut self,
        receiver: &mpsc::Receiver<RuntimeEvent>,
        hits: &x11::HitInbox,
        scheduler: &mut scheduler::Scheduler,
        runners: &mut runner::RunnerPool,
        output: &output::OutputController,
        last_diagnostics: &mut [Option<String>],
    ) -> types::Result<()> {
        loop {
            if let Some(signal) = received_signal() {
                return Err(error::AtomBlocksError::Interrupted(signal));
            }

            for index in scheduler.due(Instant::now()) {
                runners.start(index, self.config.blocks[index].clone())?;
            }

            let wait = scheduler
                .next_deadline()
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(SIGNAL_POLL_INTERVAL)
                .min(SIGNAL_POLL_INTERVAL);

            match receiver.recv_timeout(wait) {
                Ok(RuntimeEvent::Runner(result)) => {
                    let index = result.index();
                    runners.finish(index)?;
                    self.apply_run_result(result, output, last_diagnostics);
                    if let Some(index) = scheduler.complete(index) {
                        runners.start(index, self.config.blocks[index].clone())?;
                    }
                }
                Ok(RuntimeEvent::HitsReady) => {
                    for index in hits.take() {
                        if let Some(index) = scheduler.hit(index) {
                            runners.start(index, self.config.blocks[index].clone())?;
                        }
                    }
                }
                Ok(RuntimeEvent::Fatal(error)) => return Err(error),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(error::AtomBlocksError::Runtime(
                        "runtime event channel disconnected".into(),
                    ));
                }
            }
        }
    }

    fn apply_run_result(
        &mut self,
        result: runner::RunResult,
        output: &output::OutputController,
        last_diagnostics: &mut [Option<String>],
    ) {
        match result {
            runner::RunResult::Completed {
                index,
                rendered,
                diagnostic,
            } => {
                report_diagnostic(index, diagnostic.as_deref(), last_diagnostics);
                if self.cells[index] != rendered {
                    self.cells[index] = rendered;
                    output.publish(render_bar(
                        &self.cells,
                        self.config.delimiter.as_deref().unwrap_or_default(),
                    ));
                }
            }
            runner::RunResult::Failed { index, diagnostic } => {
                report_diagnostic(index, Some(&diagnostic), last_diagnostics);
            }
        }
    }
}

fn report_diagnostic(index: usize, diagnostic: Option<&str>, previous: &mut [Option<String>]) {
    let Some(slot) = previous.get_mut(index) else {
        return;
    };
    match diagnostic {
        Some(message) if slot.as_deref() != Some(message) => {
            log::error!("{message}");
            *slot = Some(message.to_owned());
        }
        Some(_) => {}
        None => *slot = None,
    }
}

/// NOTE: kept for source compatibility with 0.2.x; scheduling is now internal
#[doc(hidden)]
#[allow(dead_code)]
pub struct Task {
    block: config::Block,
    last_run: Instant,
}

fn render_bar(cells: &[String], delimiter: &str) -> String {
    cells
        .iter()
        .filter(|cell| !cell.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(delimiter)
}

static RECEIVED_SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn signal_handler(signal: i32) {
    RECEIVED_SIGNAL.store(signal, Ordering::Relaxed);
}

fn received_signal() -> Option<i32> {
    match RECEIVED_SIGNAL.load(Ordering::Relaxed) {
        0 => None,
        signal => Some(signal),
    }
}

struct SignalGuard {
    previous_int: libc::sighandler_t,
    previous_term: libc::sighandler_t,
}

impl SignalGuard {
    fn install() -> types::Result<Self> {
        RECEIVED_SIGNAL.store(0, Ordering::Relaxed);
        let handler = signal_handler as *const () as libc::sighandler_t;
        let previous_int = unsafe { libc::signal(libc::SIGINT, handler) };
        if previous_int == libc::SIG_ERR {
            return Err(std::io::Error::last_os_error().into());
        }
        let previous_term = unsafe { libc::signal(libc::SIGTERM, handler) };
        if previous_term == libc::SIG_ERR {
            unsafe {
                libc::signal(libc::SIGINT, previous_int);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            previous_int,
            previous_term,
        })
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.previous_int);
            libc::signal(libc::SIGTERM, self.previous_term);
        }
        RECEIVED_SIGNAL.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_preserves_x11_bytes_and_omits_empty_cells() {
        let cells = vec![String::new(), "[α\n]".into(), String::new(), " β ".into()];
        assert_eq!(render_bar(&cells, " | "), "[α\n] |  β ");
        assert_eq!(render_bar(&cells, ""), "[α\n] β ");
        assert_eq!(render_bar(&[], " | "), "");
        assert_eq!(render_bar(&[String::new()], " | "), "");
    }
}
