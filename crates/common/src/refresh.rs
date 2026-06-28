//! Run a job on a background thread and poll for its result, so a TUI can stay
//! responsive (and animate an indicator) while data refreshes in the background.
//!
//! This is the shared primitive behind `ctrl-r` / stale-while-revalidate: a
//! tool gives [`Refresh::new`] a closure that fetches fresh data, calls
//! [`Refresh::trigger`] to start it, and each frame calls [`Refresh::poll`] —
//! swapping in the result when it arrives. [`Refresh::in_flight`] drives the
//! "updating…" indicator.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use anyhow::{anyhow, Result};

pub struct Refresh<T> {
    job: Arc<dyn Fn() -> Result<T> + Send + Sync>,
    rx: Option<Receiver<Result<T>>>,
}

impl<T: Send + 'static> Refresh<T> {
    pub fn new(job: impl Fn() -> Result<T> + Send + Sync + 'static) -> Self {
        Self {
            job: Arc::new(job),
            rx: None,
        }
    }

    /// Start a refresh, unless one is already running.
    pub fn trigger(&mut self) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let job = Arc::clone(&self.job);
        std::thread::spawn(move || {
            let _ = tx.send(job());
        });
        self.rx = Some(rx);
    }

    /// Whether a refresh is currently running.
    pub fn in_flight(&self) -> bool {
        self.rx.is_some()
    }

    /// Non-blocking: returns the result once the job finishes, else `None`.
    pub fn poll(&mut self) -> Option<Result<T>> {
        match self.rx.as_ref()?.try_recv() {
            Ok(v) => {
                self.rx = None;
                Some(v)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                Some(Err(anyhow!("refresh task died")))
            }
        }
    }
}
