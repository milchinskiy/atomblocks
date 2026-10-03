use config::Config;
use std::{
    io::{self, Write},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{
            AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, PropMode, Property,
        },
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
                self.atoms._ATOMBLOCKS_HIT_QUEUE,
                AtomEnum::INTEGER,
                &[id],
            )?
            .check()?)
    }
}

/// Destination for bar updates. Both modes retain X11-based manual updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    X11,
    Stdout,
}

pub struct AtomBlocks {
    config: Config,
    output: OutputMode,
    cells: Vec<String>,
    xconn: Arc<x11rb::rust_connection::RustConnection>,
    root: u32,
    atoms: atoms::AtomBlocksAtoms,
}

impl AtomBlocks {
    pub fn new(config: Config) -> types::Result<Self> {
        Self::new_with_output(config, OutputMode::X11)
    }

    /// Select the bar output destination; an X11 connection is still required.
    pub fn new_with_output(config: Config, output: OutputMode) -> types::Result<Self> {
        let (xconn, root, atoms) = helpers::x11_connect()?;
        xconn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;

        xconn
            .change_window_attributes(
                root,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .expect("ChangeWindowAttributesAux");
        // Subscribe before waiting for hits, even when no WM_NAME write follows.
        xconn.flush()?;

        let capacity = config.block.len();
        let cells = vec![String::new(); capacity];
        log::trace!("Allocated {} cells ({})", capacity, cells.len());
        Ok(Self {
            config,
            output,
            cells,
            xconn: Arc::new(xconn),
            root,
            atoms,
        })
    }

    pub fn run(&mut self) -> types::Result<()> {
        let (sender, receiver) = std::sync::mpsc::channel::<Vec<usize>>();
        let sender = Arc::new(sender);
        let xconn = self.xconn.clone();
        let x11_atom = self.atoms._ATOMBLOCKS_HIT_QUEUE;
        let root = self.root;
        let x11_sender = sender.clone();
        std::thread::spawn(move || loop {
            while let Ok(Event::PropertyNotify(event)) = xconn.wait_for_event() {
                if event.atom != x11_atom || event.window != root || event.state == Property::DELETE
                {
                    continue;
                };

                let Ok(Ok(reply)) = xconn
                    .get_property(true, root, x11_atom, AtomEnum::INTEGER, 0, 1024)
                    .map(|v| v.reply())
                else {
                    log::warn!("Failed to get property reply: {:?}", event.state);
                    continue;
                };

                let Some(values) = reply.value32() else {
                    log::warn!("Failed to get reply values, continue");
                    continue;
                };

                log::info!("Received X11 PropertyNotify");
                let mut hit_request_blocks = values.map(|v| v as usize).collect::<Vec<usize>>();
                hit_request_blocks.dedup();
                hit_request_blocks.iter().for_each(|index| {
                    x11_sender
                        .send(vec![*index])
                        .map_err(|err| {
                            log::error!("send error: {:?}", err);
                        })
                        .ok();
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        });

        let mut tasks: Vec<Task> = self
            .config
            .block
            .clone()
            .iter()
            .map(|block| Task {
                block: block.clone(),
                last_run: Instant::now()
                    - Duration::from_secs_f32(block.interval.unwrap_or_default()),
            })
            .collect();

        let x11_sender = sender.clone();
        std::thread::spawn(move || loop {
            let mut indexes = Vec::new();
            for (index, task) in tasks.iter_mut().enumerate() {
                let Some(interval) = task.block.interval else {
                    continue;
                };
                if (task.last_run + Duration::from_secs_f32(interval)) > Instant::now() {
                    continue;
                }
                task.last_run = Instant::now();
                indexes.push(index);
            }

            if !indexes.is_empty() {
                x11_sender
                    .send(indexes)
                    .map_err(|err| {
                        log::error!("send error: {:?}", err);
                    })
                    .ok();
            }

            std::thread::sleep(Duration::from_millis(100));
        });

        log::info!("Ready to receive events");
        while let Ok(indexes) = receiver.recv() {
            let mut new_cells = self.cells.clone();
            let mut spawns: Vec<(usize, Child)> = Vec::new();

            for index in indexes {
                if let Some(block) = self.config.block.get(index) {
                    log::debug!("Run: {}", block.execute.as_str());

                    if let Ok(output) = Command::new("sh")
                        .arg("-c")
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .arg(block.execute.as_str())
                        .spawn()
                    {
                        spawns.push((index, output));
                    }
                }
            }

            for (index, child) in spawns {
                let Some(block) = self.config.block.get(index) else {
                    continue;
                };
                let Ok(output) = child.wait_with_output() else {
                    continue;
                };
                new_cells[index] =
                    block.print(String::from_utf8_lossy(output.stdout.as_slice()).to_string());
            }

            if new_cells != self.cells {
                self.cells = new_cells;
                self.print()?;
            }
        }

        Ok(())
    }

    fn print(&self) -> types::Result<()> {
        let result = render_bar(
            &self.cells,
            self.config.delimiter.as_deref().unwrap_or_default(),
        );
        if self.output == OutputMode::Stdout {
            write_stdout_record(&mut io::stdout().lock(), &result)?;
            return Ok(());
        }

        log::info!("Updating WM_NAME property...");
        if let Err(err) = self
            .xconn
            .change_property8(
                PropMode::REPLACE,
                self.root,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                result.as_bytes(),
            )
            .map(|r| r.check())
        {
            log::error!("Failed to set WM_NAME property: {}", err);
        }
        Ok(())
    }
}

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

fn write_stdout_record(writer: &mut impl Write, bar: &str) -> io::Result<()> {
    // A bar update is one line, even when commands or decorations contain CR/LF.
    writeln!(writer, "{}", bar.replace(['\r', '\n'], ""))?;
    writer.flush()
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

    #[derive(Default)]
    struct Writer {
        bytes: Vec<u8>,
        flushes: usize,
        write_error: Option<io::ErrorKind>,
        flush_error: Option<io::ErrorKind>,
    }

    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(kind) = self.write_error {
                return Err(io::Error::from(kind));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            match self.flush_error {
                Some(kind) => Err(io::Error::from(kind)),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn stdout_flattens_lines_preserves_text_and_flushes_each_record() {
        let mut writer = Writer::default();
        write_stdout_record(&mut writer, "[α\r\n] |  β \nnext").unwrap();
        write_stdout_record(&mut writer, "").unwrap();
        assert_eq!(writer.bytes, "[α] |  β next\n\n".as_bytes());
        assert_eq!(writer.flushes, 2);
    }

    #[test]
    fn stdout_propagates_write_and_flush_errors() {
        for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::PermissionDenied] {
            let mut writer = Writer {
                write_error: Some(kind),
                ..Writer::default()
            };
            assert_eq!(
                write_stdout_record(&mut writer, "bar").unwrap_err().kind(),
                kind
            );
            assert_eq!(writer.flushes, 0);

            let mut writer = Writer {
                flush_error: Some(kind),
                ..Writer::default()
            };
            assert_eq!(
                write_stdout_record(&mut writer, "bar").unwrap_err().kind(),
                kind
            );
            assert_eq!(writer.bytes, b"bar\n");
        }
    }
}
