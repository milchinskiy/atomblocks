use x11rb::atom_manager;

atom_manager! {
    /// A collection of Atoms.
    pub AtomBlocksAtoms:
    /// A handle to a response from the X11 server.
    AtomBlocksAtomsCookie {
        _AB_QUEUE,
    }
}
