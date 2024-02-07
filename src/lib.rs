use config::Config;
use std::{
    ops::Add,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, PropMode},
        Event,
    },
    wrapper::ConnectionExt as _,
};

pub mod atoms;
pub mod cli;
pub mod config;
pub mod error;
pub mod helpers;
pub mod types;

const LOOP_PRECISION_MS: u64 = 10;

pub struct HitMan {
    xconn: x11rb::rust_connection::RustConnection,
    root: u32,
    atoms: atoms::AtomBlocksAtoms,
}
impl HitMan {
    pub fn new() -> types::Result<Self> {
        let (xconn, root, atoms) = helpers::x11_connect()?;
        Ok(Self { xconn, root, atoms })
    }
    pub fn hit_block(&self, id: u32) -> types::Result<()> {
        Ok(self
            .xconn
            .change_property32(
                PropMode::APPEND,
                self.root,
                self.atoms._AB_QUEUE,
                AtomEnum::INTEGER,
                &[id],
            )?
            .check()?)
    }
}

pub struct AtomBlocks {
    config: Config,
    cells: Vec<String>,
    xconn: x11rb::rust_connection::RustConnection,
    root: u32,
    atoms: atoms::AtomBlocksAtoms,
}

impl AtomBlocks {
    pub fn new(config: Config) -> types::Result<Self> {
        let (xconn, root, atoms) = helpers::x11_connect()?;
        xconn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;

        let capacity = config.block.len();
        let cells = vec![String::new(); capacity];
        log::trace!("Allocated {} cells ({})", capacity, cells.len());
        Ok(Self {
            config,
            cells,
            xconn,
            root,
            atoms,
        })
    }

    pub fn run(&mut self) -> types::Result<()> {
        let mut tasks = self
            .config
            .block
            .iter()
            .enumerate()
            .map(|(index, b)| {
                let interval = Duration::from_secs_f32(b.interval.unwrap_or_default());
                Task {
                    index,
                    interval,
                    last_run: Instant::now() - interval,
                    execute: b.execute.clone(),
                }
            })
            .collect::<Vec<_>>();

        let mut last_run = Instant::now();
        loop {
            if last_run.elapsed() < Duration::from_millis(LOOP_PRECISION_MS) {
                std::thread::sleep(Duration::from_millis(LOOP_PRECISION_MS));
            }
            let hit_request_blocks: Vec<usize> =
                if let Ok(Some(Event::PropertyNotify(event))) = self.xconn.poll_for_event() {
                    log::debug!("{}: Received event: {:?}", event.window, event);
                    if event.atom != self.atoms._AB_QUEUE || event.window != self.root {
                        continue;
                    };
                    let Ok(Ok(reply)) = self
                        .xconn
                        .get_property(
                            true,
                            self.root,
                            self.atoms._AB_QUEUE,
                            AtomEnum::INTEGER,
                            0,
                            1024,
                        )
                        .map(|v| v.reply())
                    else {
                        log::debug!("Failed to get property: {:?}", event);
                        continue;
                    };
                    let Some(values) = reply.value32() else {
                        log::debug!("Failed to get values: {:?}", event);
                        continue;
                    };

                    let mut hit_request_blocks = values.map(|v| v as usize).collect::<Vec<usize>>();
                    hit_request_blocks.dedup();
                    log::debug!("Hit request blocks: {:?}", hit_request_blocks);
                    hit_request_blocks
                } else {
                    Vec::new()
                };

            let pending_tasks = tasks
                .iter_mut()
                .filter(|b| {
                    if hit_request_blocks.contains(&b.index) {
                        log::debug!("{}: Hit request to block update", b.index);
                        return true;
                    }
                    if b.interval > Duration::from_secs(0)
                        && b.last_run.add(b.interval) <= Instant::now()
                    {
                        return true;
                    }
                    false
                })
                .map(|task| {
                    log::info!("#{}: processing a request to block update", task.index);
                    task.last_run = Instant::now();
                    (
                        task.index,
                        Command::new("/bin/sh")
                            .arg("-c")
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .arg(&task.execute)
                            .spawn(),
                    )
                })
                .filter(|(_, cmd)| cmd.is_ok())
                .map(|(index, cmd)| (index, cmd.unwrap()))
                .collect::<Vec<_>>();

            let mut new_cells = self.cells.clone();
            for (index, pt) in pending_tasks {
                let Ok(output) = pt.wait_with_output() else {
                    continue;
                };
                let Some(block) = self.config.block.get(index) else {
                    continue;
                };
                log::debug!("#{}: Executed command: {}", index, block.execute);
                new_cells[index] =
                    block.print(String::from_utf8_lossy(output.stdout.as_slice()).to_string());
            }

            last_run = Instant::now();
            if new_cells != self.cells {
                self.cells = new_cells;
                self.print();
            }
        }
    }

    fn print(&self) {
        log::info!("Updating WM_NAME property...");
        let result = self.cells.iter().map(|c| c.to_string());
        let delim = self.config.delimiter.clone().unwrap_or_default();
        let result = result
            .into_iter()
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>();

        if let Err(err) = self
            .xconn
            .change_property8(
                PropMode::REPLACE,
                self.root,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                result.join(delim.as_str()).as_bytes(),
            )
            .map(|r| r.check())
        {
            log::error!("Failed to set WM_NAME property: {}", err);
        }
    }
}

pub struct Task {
    index: usize,
    interval: Duration,
    last_run: Instant,
    execute: String,
}
