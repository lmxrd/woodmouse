//! Limits on guessing: keyed rate limiters (governor, GCRA) on every attempt, not only failed ones, in
//! memory only (the address is never stored). The quotas are potoo's:
//!
//! - per email (lowercase) of a password sign-in: a burst of 5, then 1 a minute;
//! - per client address (`TRUST_PROXY`-aware, see `routes::auth::ClientAddr`) of a password sign-in, the
//!   two-factor code step and two-factor setup at sign-in: a burst of 30, then 1 every 6 seconds;
//! - per user, for two-factor codes (the code step, setup, and every settings action that checks a code)
//!   and, separately, for password changes: a burst of 5, then 1 a minute.
//!
//! Over a limit the answer is 429 with `Retry-After` (seconds). `jobs` calls [`Limits::retain_recent`]
//! now and then, so keys that have refilled are forgotten and memory stays bounded.

use std::num::NonZeroU32;
use std::time::Duration;

use governor::clock::{Clock, DefaultClock};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use crate::error::ApiError;

/// What a user-keyed limit is for: each has its own budget.
#[derive(Clone, Copy, Debug)]
pub enum UserAction {
    /// A two-factor code or recovery code.
    Code,
    /// Changing the password.
    Password,
}

pub struct Limits {
    by_email: DefaultKeyedRateLimiter<String>,
    by_address: DefaultKeyedRateLimiter<String>,
    by_user: DefaultKeyedRateLimiter<String>,
}

fn quota(burst: u32, refill_every: Duration) -> Quota {
    Quota::with_period(refill_every)
        .expect("a period above zero")
        .allow_burst(NonZeroU32::new(burst).expect("a burst above zero"))
}

impl Default for Limits {
    /// The quotas in force (see the module docs).
    fn default() -> Self {
        Self::with_quotas(
            quota(5, Duration::from_secs(60)),
            quota(30, Duration::from_secs(6)),
            quota(5, Duration::from_secs(60)),
        )
    }
}

impl Limits {
    /// Other quotas: per email, per address, per user.
    pub fn with_quotas(email: Quota, address: Quota, user: Quota) -> Self {
        Self {
            by_email: RateLimiter::keyed(email),
            by_address: RateLimiter::keyed(address),
            by_user: RateLimiter::keyed(user),
        }
    }

    /// So generous that nothing a test does reaches them (tests that check a limit use the default).
    pub fn generous() -> Self {
        let wide = quota(10_000, Duration::from_millis(1));
        Self::with_quotas(wide, wide, wide)
    }

    fn check(limiter: &DefaultKeyedRateLimiter<String>, key: String) -> Result<(), ApiError> {
        limiter.check_key(&key).map_err(|not_until| {
            let wait = not_until.wait_time_from(DefaultClock::default().now());
            // Whole seconds, rounded up: `Retry-After: 0` would invite an immediate retry.
            ApiError::TooManyRequests {
                retry_after: wait.as_secs() + u64::from(wait.subsec_nanos() > 0),
            }
        })
    }

    /// A password sign-in for `email`.
    pub fn email(&self, email: &str) -> Result<(), ApiError> {
        Self::check(&self.by_email, email.trim().to_lowercase())
    }

    /// An attempt from `address`; nothing to count when it is unknown.
    pub fn address(&self, address: Option<&str>) -> Result<(), ApiError> {
        match address {
            Some(address) => Self::check(&self.by_address, address.to_string()),
            None => Ok(()),
        }
    }

    /// An attempt by `user_id` at `action`.
    pub fn user(&self, action: UserAction, user_id: uuid::Uuid) -> Result<(), ApiError> {
        Self::check(&self.by_user, format!("{action:?}:{user_id}"))
    }

    /// Forget the keys that have refilled completely.
    pub fn retain_recent(&self) {
        self.by_email.retain_recent();
        self.by_address.retain_recent();
        self.by_user.retain_recent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quotas_are_potoos() {
        let limits = Limits::default();
        for _ in 0..5 {
            limits.email("A@b.c").unwrap();
        }
        // The same email in another case counts too, and the answer says when to retry.
        match limits.email("a@B.c") {
            Err(ApiError::TooManyRequests { retry_after }) => {
                assert!((1..=60).contains(&retry_after), "{retry_after}")
            }
            other => panic!("{other:?}"),
        }
        limits.email("other@b.c").unwrap();

        for _ in 0..30 {
            limits.address(Some("1.2.3.4")).unwrap();
        }
        assert!(limits.address(Some("1.2.3.4")).is_err());
        assert!(limits.address(None).is_ok());

        let user = uuid::Uuid::new_v4();
        for _ in 0..5 {
            limits.user(UserAction::Code, user).unwrap();
        }
        assert!(limits.user(UserAction::Code, user).is_err());
        // A separate budget for password changes.
        limits.user(UserAction::Password, user).unwrap();
        limits.retain_recent();
    }
}
