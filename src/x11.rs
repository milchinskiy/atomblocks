use crate::atoms::AtomBlocksAtoms;
use crate::RuntimeEvent;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Property},
        Event,
    },
    rust_connection::RustConnection as Conn,
};

const IDS_PER_CHUNK: u32 = 1024;
const CHUNKS_PER_PASS: usize = 16;

pub fn x11_connect() -> crate::types::Result<(Conn, u32, AtomBlocksAtoms)> {
    let (xconn, _screen_id) = x11rb::connect(None)?;
    let root = xconn.setup().roots[_screen_id].root;
    let atoms = AtomBlocksAtoms::new(&xconn)?.reply()?;
    Ok((xconn, root, atoms))
}

struct HitState {
    ids: HashSet<usize>,
    notified: bool,
}

pub(crate) struct HitInbox {
    block_count: usize,
    state: Mutex<HitState>,
}

impl HitInbox {
    pub fn new(block_count: usize) -> Self {
        Self {
            block_count,
            state: Mutex::new(HitState {
                ids: HashSet::with_capacity(block_count),
                notified: false,
            }),
        }
    }

    fn push<I>(&self, ids: I, events: &Sender<RuntimeEvent>)
    where
        I: IntoIterator<Item = u32>,
    {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut changed = false;
        for id in ids {
            let index = id as usize;
            if index >= self.block_count {
                log::warn!("ignoring hit for unknown block {id}");
                continue;
            }
            changed |= state.ids.insert(index);
        }

        if changed && !state.notified {
            state.notified = true;
            if events.send(RuntimeEvent::HitsReady).is_err() {
                state.notified = false;
            }
        }
    }

    pub fn take(&self) -> Vec<usize> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.notified = false;
        state.ids.drain().collect()
    }
}

pub struct Backend {
    connection: Option<x11rb::rust_connection::RustConnection>,
    root: u32,
    atom: u32,
    thread: Option<JoinHandle<()>>,
}

impl Backend {
    pub fn connect() -> crate::types::Result<Self> {
        let (connection, root, atoms) = x11_connect()?;
        connection
            .change_window_attributes(
                root,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?
            .check()?;
        connection.flush()?;

        Ok(Self {
            connection: Some(connection),
            root,
            atom: atoms._ATOMBLOCKS_HIT_QUEUE,
            thread: None,
        })
    }

    pub fn start(
        &mut self,
        inbox: Arc<HitInbox>,
        events: Sender<RuntimeEvent>,
        shutdown: Arc<AtomicBool>,
    ) -> crate::types::Result<()> {
        if self.thread.is_some() {
            return Err(crate::error::AtomBlocksError::Runtime(
                "X11 backend is already running".into(),
            ));
        }
        let Some(connection) = self.connection.take() else {
            return Err(crate::error::AtomBlocksError::Runtime(
                "X11 backend cannot be restarted".into(),
            ));
        };
        let root = self.root;
        let atom = self.atom;
        self.thread = Some(thread::Builder::new().name("atomblocks-x11".into()).spawn(
            move || {
                if let Err(error) = run_listener(connection, root, atom, &inbox, &events, &shutdown)
                {
                    if !shutdown.load(Ordering::Acquire) {
                        let _ = events.send(RuntimeEvent::Fatal(error));
                    }
                }
            },
        )?);
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("X11 thread panicked during shutdown");
            }
        }
    }
}

fn run_listener(
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
    atom: u32,
    inbox: &HitInbox,
    events: &Sender<RuntimeEvent>,
    shutdown: &AtomicBool,
) -> crate::types::Result<()> {
    let mut offset = 0_u32;
    let mut draining = true;

    while !shutdown.load(Ordering::Acquire) {
        if draining {
            let drain = drain_hits(&connection, root, atom, offset, inbox, events)?;
            offset = drain.next_offset;
            draining = drain.more;
            if draining {
                thread::yield_now();
                continue;
            }
        }

        let mut saw_event = false;
        loop {
            match connection.poll_for_event()? {
                Some(Event::PropertyNotify(event))
                    if event.atom == atom
                        && event.window == root
                        && event.state != Property::DELETE =>
                {
                    draining = true;
                    saw_event = true;
                }
                Some(_) => saw_event = true,
                None => break,
            }
        }

        if !saw_event && !draining {
            thread::sleep(Duration::from_millis(20));
        }
    }

    Ok(())
}

struct DrainResult {
    more: bool,
    next_offset: u32,
}

fn drain_hits(
    connection: &x11rb::rust_connection::RustConnection,
    root: u32,
    atom: u32,
    mut offset: u32,
    inbox: &HitInbox,
    events: &Sender<RuntimeEvent>,
) -> crate::types::Result<DrainResult> {
    for _ in 0..CHUNKS_PER_PASS {
        let reply = connection
            .get_property(true, root, atom, AtomEnum::ANY, offset, IDS_PER_CHUNK)?
            .reply()?;

        if reply.format == 0 {
            return Ok(DrainResult {
                more: false,
                next_offset: 0,
            });
        }
        if reply.type_ != u32::from(AtomEnum::INTEGER) || reply.format != 32 {
            log::warn!("discarding malformed _ATOMBLOCKS_HIT_QUEUE property");
            connection.delete_property(root, atom)?.check()?;
            return Ok(DrainResult {
                more: false,
                next_offset: 0,
            });
        }

        let Some(values) = reply.value32() else {
            return Err(crate::error::AtomBlocksError::Runtime(
                "failed to decode _ATOMBLOCKS_HIT_QUEUE".into(),
            ));
        };
        let ids = values.collect::<Vec<_>>();
        if reply.bytes_after > 0 && ids.is_empty() {
            return Err(crate::error::AtomBlocksError::Runtime(
                "X11 hit queue made no progress".into(),
            ));
        }
        offset = offset.saturating_add(ids.len() as u32);
        inbox.push(ids, events);

        if reply.bytes_after == 0 {
            return Ok(DrainResult {
                more: false,
                next_offset: 0,
            });
        }
    }

    Ok(DrainResult {
        more: true,
        next_offset: offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn hit_inbox_deduplicates_and_bounds_pending_ids() {
        let (sender, receiver) = mpsc::channel();
        let inbox = HitInbox::new(2);
        inbox.push([0, 1, 0, 9], &sender);
        assert!(matches!(receiver.recv().unwrap(), RuntimeEvent::HitsReady));
        let mut hits = inbox.take();
        hits.sort_unstable();
        assert_eq!(hits, vec![0, 1]);
        assert!(receiver.try_recv().is_err());
    }
}
