//! Waiting for several jobs, but never longer than a limit.
//!
//! Asking every desktop how it is runs one job per desktop. One that never
//! answers must not hold up the others, or the interface waiting for all of
//! them: the answers that came in time are used, and the rest are told "no
//! answer in time". A late job is left to finish on its own thread.

use std::sync::{mpsc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

/// Take a lock, waiting at most `wait` for whoever holds it. `None` when it is
/// still held then. For work that must not run twice at once but also must
/// not make the caller wait for ever on a holder stuck on the network. A lock
/// left poisoned by a panic is taken as it is.
pub fn lock_within<T>(lock: &Mutex<T>, wait: Duration) -> Option<MutexGuard<'_, T>> {
    let started = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(e)) => return Some(e.into_inner()),
            Err(TryLockError::WouldBlock) if started.elapsed() < wait => std::thread::sleep(Duration::from_millis(10).min(wait)),
            Err(TryLockError::WouldBlock) => return None,
        }
    }
}

/// A job to run on a thread of its own.
pub type Job<T> = Box<dyn FnOnce() -> T + Send + 'static>;

/// Run every job at once and wait for them all, but no longer than `limit` in
/// total. The result has one entry per job, in the order given: `None` for a
/// job that did not finish in time (or panicked). Jobs that are late keep
/// running in the background and their results are dropped.
pub fn collect_until<T: Send + 'static>(jobs: Vec<Job<T>>, limit: Duration) -> Vec<Option<T>> {
    let count = jobs.len();
    let (sender, receiver) = mpsc::channel();
    for (index, job) in jobs.into_iter().enumerate() {
        let sender = sender.clone();
        std::thread::spawn(move || {
            // The receiver is gone when the job was late: nobody wants it.
            let _ = sender.send((index, job()));
        });
    }
    drop(sender);
    let deadline = Instant::now() + limit;
    let mut out: Vec<Option<T>> = (0..count).map(|_| None).collect();
    let mut got = 0;
    while got < count {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((index, value)) => {
                out[index] = Some(value);
                got += 1;
            }
            // Out of time, or every remaining job panicked.
            Err(_) => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job<T: Send + 'static>(ms: u64, value: T) -> Job<T> {
        Box::new(move || {
            std::thread::sleep(Duration::from_millis(ms));
            value
        })
    }

    #[test]
    fn everything_that_is_quick_comes_back_in_order() {
        let out = collect_until(vec![job(30, "a"), job(0, "b"), job(10, "c")], Duration::from_secs(5));
        assert_eq!(out, vec![Some("a"), Some("b"), Some("c")]);
    }

    #[test]
    fn no_jobs_is_no_waiting() {
        let started = Instant::now();
        assert!(collect_until::<u8>(Vec::new(), Duration::from_secs(5)).is_empty());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_slow_job_is_given_up_on_at_the_limit_and_the_rest_are_kept() {
        let started = Instant::now();
        let out = collect_until(vec![job(0, 1), job(3000, 2), job(20, 3)], Duration::from_millis(300));
        let took = started.elapsed();
        assert_eq!(out, vec![Some(1), None, Some(3)]);
        assert!(took >= Duration::from_millis(250), "waited for the limit: {took:?}");
        assert!(took < Duration::from_millis(1500), "did not wait for the slow one: {took:?}");
    }

    #[test]
    fn the_limit_is_shared_not_added_up() {
        // Five jobs of 200 ms each, run at once, fit in a 1 s limit.
        let started = Instant::now();
        let out = collect_until((0..5).map(|i| job(200, i)).collect(), Duration::from_secs(1));
        assert!(out.iter().all(Option::is_some));
        assert!(started.elapsed() < Duration::from_millis(900));
    }

    #[test]
    fn a_late_job_still_finishes_in_the_background() {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        let late: Job<()> = Box::new(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let out = collect_until(vec![late], Duration::from_millis(50));
        assert_eq!(out, vec![None]);
        assert!(!done.load(std::sync::atomic::Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(600));
        assert!(done.load(std::sync::atomic::Ordering::SeqCst), "left to finish");
    }

    #[test]
    fn a_lock_that_stays_held_is_given_up_on_at_the_limit() {
        let lock = Mutex::new(());
        let _held = lock.lock().unwrap();
        let started = Instant::now();
        assert!(lock_within(&lock, Duration::from_millis(150)).is_none());
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(140) && took < Duration::from_secs(2), "{took:?}");
    }

    #[test]
    fn a_lock_let_go_in_time_is_taken() {
        let lock = std::sync::Arc::new(Mutex::new(0));
        let held = lock.clone();
        let holder = std::thread::spawn(move || {
            let _g = held.lock().unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(lock_within(&lock, Duration::from_secs(5)).is_some());
        holder.join().unwrap();
    }

    #[test]
    fn a_poisoned_lock_is_still_taken() {
        let lock = std::sync::Arc::new(Mutex::new(()));
        let l = lock.clone();
        let _ = std::thread::spawn(move || {
            let _g = l.lock().unwrap();
            panic!("holder fails");
        })
        .join();
        assert!(lock_within(&lock, Duration::from_millis(50)).is_some());
    }

    #[test]
    fn passes_that_wait_for_the_lock_never_overlap() {
        use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
        let lock = std::sync::Arc::new(Mutex::new(()));
        let inside = std::sync::Arc::new(AtomicU32::new(0));
        let most = std::sync::Arc::new(AtomicU32::new(0));
        let passes: Vec<_> = (0..6)
            .map(|_| {
                let (lock, inside, most) = (lock.clone(), inside.clone(), most.clone());
                std::thread::spawn(move || {
                    let Some(_turn) = lock_within(&lock, Duration::from_secs(10)) else { return };
                    let now = inside.fetch_add(1, SeqCst) + 1;
                    most.fetch_max(now, SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    inside.fetch_sub(1, SeqCst);
                })
            })
            .collect();
        passes.into_iter().for_each(|p| p.join().unwrap());
        assert_eq!(most.load(SeqCst), 1, "one pass at a time");
    }

    #[test]
    fn a_job_that_panics_is_just_late() {
        let bad: Job<u8> = Box::new(|| panic!("a job that fails"));
        let started = Instant::now();
        let out = collect_until(vec![bad, job(0, 7)], Duration::from_secs(5));
        assert_eq!(out, vec![None, Some(7)]);
        assert!(started.elapsed() < Duration::from_secs(2), "a panic does not cost the whole limit");
    }
}
