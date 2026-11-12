//! Bounded object scheduling. Every started worker joins before staging can drop.
use crate::Options;
use anyhow::{ensure, Result};
use std::{
    io::{self, Read},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub(crate) fn check(options: &Options, stopped: &AtomicBool) -> Result<()> {
    options.check()?;
    ensure!(
        !stopped.load(Ordering::Acquire),
        "LFS transfer stopped after another object failed"
    );
    Ok(())
}

pub(crate) struct Reader<T> {
    pub inner: T,
    pub options: Options,
    pub stopped: Arc<AtomicBool>,
}
impl<T: Read> Read for Reader<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        check(&self.options, &self.stopped).map_err(io::Error::other)?;
        let n = self.inner.read(buffer)?;
        check(&self.options, &self.stopped).map_err(io::Error::other)?;
        Ok(n)
    }
}

pub(crate) fn run<T: Sync>(
    items: &[T],
    options: &Options,
    apply: impl Fn(&T, &Arc<AtomicBool>) -> Result<()> + Sync,
) -> Result<()> {
    ensure!(
        (1..=4).contains(&options.lfs_workers),
        "LFS workers must be 1..4"
    );
    options.check()?;
    let stopped = Arc::new(AtomicBool::new(false));
    if options.lfs_workers == 1 {
        for item in items {
            check(options, &stopped)?;
            apply(item, &stopped)?;
        }
        return options.check();
    }
    struct State {
        next: usize,
        error: Option<anyhow::Error>,
    }
    let state = Mutex::new(State {
        next: 0,
        error: None,
    });
    let fail = |error| {
        let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
        if state.error.is_none() {
            state.error = Some(error);
        }
        stopped.store(true, Ordering::Release);
    };
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..options.lfs_workers.min(items.len()) {
            let worker = || {
                let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
                    loop {
                        let index = {
                            let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
                            if state.error.is_some() {
                                return Ok(());
                            }
                            options.check()?;
                            if state.next == items.len() {
                                return Ok(());
                            }
                            let index = state.next;
                            state.next += 1;
                            index
                        };
                        check(options, &stopped)?;
                        apply(&items[index], &stopped)?;
                    }
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("LFS worker stopped unexpectedly")));
                if let Err(error) = result {
                    fail(error);
                }
            };
            match std::thread::Builder::new()
                .name("rs-lfs".into())
                .spawn_scoped(scope, worker)
            {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    fail(anyhow::anyhow!("Cannot start bounded LFS worker"));
                    break;
                }
            }
        }
        for handle in handles {
            if handle.join().is_err() {
                fail(anyhow::anyhow!("LFS worker stopped unexpectedly"));
            }
        }
    });
    if let Some(error) = state.into_inner().unwrap_or_else(|p| p.into_inner()).error {
        return Err(error);
    }
    options.check()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{atomic::AtomicUsize, Barrier},
        time::Duration,
    };
    #[test]
    fn bounds_workers_and_joins_before_return() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let completed = AtomicUsize::new(0);
        let barrier = Barrier::new(4);
        let options = Options {
            lfs_workers: 4,
            ..Options::default()
        };
        run(&(0..24).collect::<Vec<_>>(), &options, |index, _| {
            let n = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(n, Ordering::SeqCst);
            if *index < 4 {
                barrier.wait();
            }
            active.fetch_sub(1, Ordering::SeqCst);
            completed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert!((2..=4).contains(&peak.load(Ordering::SeqCst)));
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(completed.load(Ordering::SeqCst), 24);
    }
    #[test]
    fn concurrent_failure_drains_started_workers_and_leaves_queue_untouched() {
        let options = Options {
            lfs_workers: 4,
            ..Options::default()
        };
        let active = AtomicUsize::new(0);
        let started = AtomicUsize::new(0);
        let barrier = Barrier::new(4);
        assert!(
            run(&(0..20).collect::<Vec<_>>(), &options, |index, stopped| {
                started.fetch_add(1, Ordering::SeqCst);
                active.fetch_add(1, Ordering::SeqCst);
                if *index < 4 {
                    barrier.wait();
                }
                if *index == 0 {
                    active.fetch_sub(1, Ordering::SeqCst);
                    anyhow::bail!("synthetic object failure");
                }
                while !stopped.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(started.load(Ordering::SeqCst), 4);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(!options.cancelled.load(Ordering::SeqCst));
    }
    #[test]
    fn failure_stops_queue_without_turning_into_user_cancellation() {
        let options = Options {
            lfs_workers: 1,
            ..Options::default()
        };
        let started = AtomicUsize::new(0);
        assert!(run(&[0, 1, 2], &options, |n, _| {
            started.fetch_add(1, Ordering::SeqCst);
            ensure!(*n != 1, "synthetic failure");
            Ok(())
        })
        .is_err());
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert!(!options.cancelled.load(Ordering::SeqCst));
        options.cancelled.store(true, Ordering::SeqCst);
        assert!(run(&[0], &options, |_, _| panic!(
            "cancelled queue started work"
        ))
        .is_err());
        let invalid = Options {
            lfs_workers: 5,
            ..Options::default()
        };
        assert!(run(&[0], &invalid, |_, _| Ok(())).is_err());
    }
}
