use std::time::{Duration, Instant};

pub struct RecoveryThrottle {
    next_attempt_at: Instant,
}

impl RecoveryThrottle {
    pub fn new(now: Instant) -> Self {
        Self {
            next_attempt_at: now,
        }
    }

    pub fn acquire(&mut self, now: Instant, retry_interval: Duration) -> bool {
        if now < self.next_attempt_at {
            return false;
        }
        self.next_attempt_at = now + retry_interval;
        true
    }

    pub fn reset(&mut self, now: Instant) {
        self.next_attempt_at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_attempts_are_throttled_until_the_retry_interval() {
        let start = Instant::now();
        let retry = Duration::from_secs(30);
        let mut throttle = RecoveryThrottle::new(start);

        assert!(throttle.acquire(start, retry));
        assert!(!throttle.acquire(start + Duration::from_secs(29), retry));
        assert!(throttle.acquire(start + retry, retry));
    }

    #[test]
    fn healthy_status_resets_the_recovery_throttle() {
        let start = Instant::now();
        let retry = Duration::from_secs(30);
        let mut throttle = RecoveryThrottle::new(start);
        assert!(throttle.acquire(start, retry));

        let healthy = start + Duration::from_secs(5);
        throttle.reset(healthy);

        assert!(throttle.acquire(healthy, retry));
    }
}
