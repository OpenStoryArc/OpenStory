//! Process-wide bus health facts that live outside the client connection
//! (E-07). `nats_child_alive` is set false by the managed NATS watcher when
//! the child process exits; `/api/health` reads it so `bus.connected` can
//! say no even while the client still holds a socket.

use std::sync::atomic::{AtomicBool, Ordering};

static NATS_CHILD_ALIVE: AtomicBool = AtomicBool::new(true);

/// True unless a managed NATS child has been seen to exit.
pub fn nats_child_alive() -> bool {
    NATS_CHILD_ALIVE.load(Ordering::SeqCst)
}

pub fn set_nats_child_alive(alive: bool) {
    NATS_CHILD_ALIVE.store(alive, Ordering::SeqCst);
}
