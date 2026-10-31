//! Work done off the UI thread. Each job has a key, so the UI can tell what a result is for when
//! it collects it with [`Jobs::next`].

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Wakes the UI, from the thread a job finished on, so it picks the result up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Runs a job somewhere other than the calling thread.
type Spawn = fn(Box<dyn FnOnce() + Send>);

pub struct Jobs<K, T> {
    spawn: Spawn,
    tx: Sender<(K, T)>,
    rx: Receiver<(K, T)>,
    /// Started but not yet collected.
    running: usize,
    waker: Option<Waker>,
}

impl<K: Send + 'static, T: Send + 'static> Jobs<K, T> {
    /// Jobs share rayon's pool, so a big batch only runs a few at once.
    pub fn on_pool() -> Self {
        Self::new(rayon::spawn)
    }

    /// Each job gets a thread of its own, so it starts straight away whatever else is running.
    pub fn on_own_threads() -> Self {
        Self::new(|job| {
            std::thread::spawn(job);
        })
    }

    fn new(spawn: Spawn) -> Self {
        let (tx, rx) = channel();
        Self {
            spawn,
            tx,
            rx,
            running: 0,
            waker: None,
        }
    }

    /// Calls `waker` whenever a job finishes, from the thread it ran on.
    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    pub fn spawn(&mut self, key: K, work: impl FnOnce() -> T + Send + 'static) {
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        self.running += 1;
        (self.spawn)(Box::new(move || {
            // The receiver goes when the app does, or when another project replaces its jobs.
            tx.send((key, work())).ok();
            if let Some(waker) = waker {
                waker();
            }
        }));
    }

    /// The next finished job's key and result, if any.
    pub fn next(&mut self) -> Option<(K, T)> {
        let done = self.rx.try_recv().ok()?;
        self.running -= 1;
        Some(done)
    }

    /// How many jobs have been started and not collected.
    pub fn running(&self) -> usize {
        self.running
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    /// By the clock rather than counting tries, and generously: the pool is shared with every
    /// other test running at once.
    fn wait_for<K: Send + 'static, T: Send + 'static>(jobs: &mut Jobs<K, T>) -> (K, T) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Some(done) = jobs.next() {
                return done;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("job never finished");
    }

    #[test]
    fn results_come_back_with_their_keys_and_wake_the_ui() {
        for mut jobs in [Jobs::on_pool(), Jobs::on_own_threads()] {
            let woken = Arc::new(AtomicUsize::new(0));
            let counter = woken.clone();
            jobs.set_waker(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }));
            jobs.spawn("a", || 1);
            assert_eq!(jobs.running(), 1);
            assert_eq!(wait_for(&mut jobs), ("a", 1));
            assert_eq!(jobs.running(), 0);
            assert_eq!(woken.load(Ordering::SeqCst), 1);
            assert!(jobs.next().is_none());
        }
    }
}
