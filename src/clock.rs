/// A UTC instant, stored as whole seconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Builds an instant from a count of seconds since the Unix epoch.
    pub const fn from_unix_seconds(seconds: i64) -> Self {
        Self(seconds)
    }

    /// Seconds since the Unix epoch, negative before it.
    pub const fn unix_seconds(self) -> i64 {
        self.0
    }
}

/// Source of the current UTC instant, injected so the engine stays pure.
pub trait Clock {
    /// The instant this clock reads now.
    fn now(&self) -> Timestamp;
}

/// A clock frozen on one instant, used by unit tests and scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock {
    now: Timestamp,
}

impl FixedClock {
    /// Builds a clock that always reads `now`.
    pub const fn new(now: Timestamp) -> Self {
        Self { now }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FixedClock, Timestamp};

    #[test]
    fn a_timestamp_keeps_the_seconds_it_was_built_from() {
        assert_eq!(Timestamp::from_unix_seconds(-5).unix_seconds(), -5);
        assert_eq!(Timestamp::from_unix_seconds(0).unix_seconds(), 0);
    }

    #[test]
    fn timestamps_order_on_their_seconds() {
        let older = Timestamp::from_unix_seconds(10);
        let newer = Timestamp::from_unix_seconds(20);
        assert!(newer > older);
        assert_eq!(older, Timestamp::from_unix_seconds(10));
    }

    #[test]
    fn a_fixed_clock_reads_the_same_instant_every_time() {
        let clock = FixedClock::new(Timestamp::from_unix_seconds(1_700_000_000));
        assert_eq!(clock.now(), Timestamp::from_unix_seconds(1_700_000_000));
        assert_eq!(clock.now(), clock.now());
    }
}
