//! Locks out guessing: a name that fails to authenticate too often is refused for a while.
//!
//! Failures are counted by what is being guessed, a username or a client id, so an attack spread
//! over many addresses still counts. It also means someone can lock a known name out: the lock
//! is short, and a correct password does not lift it. Counts live in the memory of the instance.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::LockoutConfig;

/// Names tracked at a time. Past this, names already expired go first, and new ones wait.
const MAX_TRACKED: usize = 100_000;

#[derive(Debug, Clone, Copy)]
struct Failures {
    count: u32,
    /// When the count starts over, or a lock ends.
    until: Instant,
}

/// The failed attempts of the names being guessed.
#[derive(Debug)]
pub struct Lockout {
    config: LockoutConfig,
    failures: Mutex<HashMap<String, Failures>>,
}

impl Lockout {
    pub fn new(config: LockoutConfig) -> Self {
        Self {
            config,
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Failures>> {
        self.failures.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How long `name` is locked out for, if it is.
    pub fn remaining(&self, name: &str) -> Option<Duration> {
        self.remaining_at(name, Instant::now())
    }

    fn remaining_at(&self, name: &str, now: Instant) -> Option<Duration> {
        if self.config.max_failed_attempts == 0 {
            return None;
        }
        let failures = self.lock();
        let entry = failures.get(name)?;
        (entry.count >= self.config.max_failed_attempts && entry.until > now)
            .then(|| entry.until - now)
    }

    /// Count a failed attempt.
    pub fn failed(&self, name: &str) {
        self.failed_at(name, Instant::now());
    }

    fn failed_at(&self, name: &str, now: Instant) {
        if self.config.max_failed_attempts == 0 {
            return;
        }
        let mut failures = self.lock();
        if failures.len() >= MAX_TRACKED {
            failures.retain(|_, entry| entry.until > now);
            if failures.len() >= MAX_TRACKED && !failures.contains_key(name) {
                return;
            }
        }
        let window = self.config.duration;
        let entry = failures.entry(name.to_owned()).or_insert(Failures {
            count: 0,
            until: now + window,
        });
        if entry.until <= now {
            entry.count = 0;
        }
        entry.count += 1;
        entry.until = now + window;
    }

    /// A success forgets the failures.
    pub fn succeeded(&self, name: &str) {
        self.lock().remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lockout(max_failed_attempts: u32) -> Lockout {
        Lockout::new(LockoutConfig {
            max_failed_attempts,
            duration: Duration::from_secs(60),
        })
    }

    #[test]
    fn a_name_is_locked_at_the_limit_and_alone() {
        let lockout = lockout(3);
        let now = Instant::now();
        lockout.failed_at("a", now);
        lockout.failed_at("a", now);
        assert_eq!(lockout.remaining_at("a", now), None);
        lockout.failed_at("a", now);
        assert_eq!(
            lockout.remaining_at("a", now),
            Some(Duration::from_secs(60))
        );
        assert_eq!(lockout.remaining_at("b", now), None);
    }

    #[test]
    fn a_lock_ends_and_the_count_starts_over() {
        let lockout = lockout(2);
        let now = Instant::now();
        lockout.failed_at("a", now);
        lockout.failed_at("a", now);
        let later = now + Duration::from_secs(61);
        assert_eq!(lockout.remaining_at("a", later), None);
        lockout.failed_at("a", later);
        assert_eq!(lockout.remaining_at("a", later), None);
    }

    #[test]
    fn a_success_forgets_the_failures() {
        let lockout = lockout(2);
        let now = Instant::now();
        lockout.failed_at("a", now);
        lockout.succeeded("a");
        lockout.failed_at("a", now);
        assert_eq!(lockout.remaining_at("a", now), None);
    }

    #[test]
    fn zero_attempts_turns_it_off() {
        let lockout = lockout(0);
        let now = Instant::now();
        for _ in 0..10 {
            lockout.failed_at("a", now);
        }
        assert_eq!(lockout.remaining_at("a", now), None);
    }
}
