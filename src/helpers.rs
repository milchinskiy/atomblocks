use crate::atoms::AtomBlocksAtoms;
use x11rb::{connection::Connection, rust_connection::RustConnection as Conn};

pub fn x11_connect() -> crate::types::Result<(Conn, u32, AtomBlocksAtoms)> {
    let (xconn, _screen_id) = x11rb::connect(None)?;
    let root = xconn.setup().roots[_screen_id].root;
    let atoms = AtomBlocksAtoms::new(&xconn)?.reply()?;
    Ok((xconn, root, atoms))
}
