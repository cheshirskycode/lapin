use crate::Result;
use std::{
    sync::{Arc, Mutex, MutexGuard},
    thread,
};

pub type JoinHandle = thread::JoinHandle<Result<()>>;
type Inner = Option<JoinHandle>;

#[derive(Clone)]
pub struct ThreadHandle(Arc<Mutex<Inner>>);

impl Default for ThreadHandle {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
}

impl ThreadHandle {
    pub(crate) fn register(&self, handle: JoinHandle) {
        *self.lock_inner() = Some(handle);
    }

    fn take(&self) -> Option<JoinHandle> {
        self.lock_inner().take()
    }

    pub(crate) fn wait(&self, context: &'static str) -> Result<()> {
        if let Some(handle) = self.take() {
            if handle.thread().id() != thread::current().id() {
                handle.join().expect(context)?;
            }
        }
        Ok(())
    }

    fn lock_inner(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}
