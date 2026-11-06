//! Two-factor sign-in: a time-based one-time password (TOTP, RFC 6238: HMAC-SHA1, 30-second steps, six
//! digits, what every authenticator app speaks) and single-use recovery codes.
//!
//! The secret is in `user_totp` (one row per user; `enabled_at` is `None` while it is being set up), with
//! `last_step`, the newest time step a code was accepted for: a code is refused for that step or an older
//! one, so a code seen over someone's shoulder cannot be used again. Recovery codes are random (80 bits) and
//! kept as SHA-256 hashes in `user_recovery_codes`; each works once.
//!
//! A sign-in (password or single sign-on) of someone with two-factor on gets no session: it gets a
//! short-lived challenge ([`Challenge`]) in a signed cookie, and the session comes with the code
//! (`routes::auth`). With `TOTP_REQUIRED`, someone without it gets an `Enroll` challenge instead and sets it
//! up before the session ([`second_step`]).

use std::collections::HashSet;

use axum_extra::extract::cookie::{Cookie, Key, SameSite, SignedCookieJar};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Builder, Secret, Totp};

use crate::db::Db;
use crate::db::prelude::*;
use crate::error::ApiError;
use crate::time;
use entity::{UserTotp, user_recovery_codes, user_totp};
use sea_orm::Condition;

/// The issuer shown in authenticator apps.
pub const ISSUER: &str = "Woodmouse";
/// Length of a time step.
pub const STEP_SECS: u64 = 30;
/// Digits in a code.
pub const DIGITS: u8 = 6;
/// Recovery codes made at a time.
pub const RECOVERY_CODES: usize = 10;
/// How long the second step of a sign-in may take after the password.
pub const CHALLENGE_TTL_SECS: i64 = 5 * 60;
/// Bytes in a secret: 160 bits, the length RFC 4226 recommends for HMAC-SHA1.
const SECRET_BYTES: usize = 20;

/// The generator for a secret (raw bytes): SHA-1, six digits, 30-second steps, and one step either side
/// accepted (clocks drift, people type slowly). `account` (the email) and the issuer are only for the
/// `otpauth://` URL.
fn generator(secret: Vec<u8>, account: &str) -> Result<Totp, ApiError> {
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(DIGITS)
        .with_skew(1)
        .with_step_duration(STEP_SECS)
        .with_secret(secret)
        .with_account_name(account)
        .with_issuer(Some(ISSUER))
        .build()
        .map_err(|_| ApiError::bad_request("That account cannot be used in an authenticator app"))
}

/// The generator for a stored (base32) secret.
fn stored(secret: &str) -> Option<Totp> {
    let bytes = Secret::try_from_base32(secret).ok()?.as_bytes().to_vec();
    generator(bytes, "").ok()
}

pub fn now_unix() -> u64 {
    jiff::Timestamp::now().as_second().max(0) as u64
}

/// The step now.
pub fn current_step() -> u64 {
    now_unix() / STEP_SECS
}

/// The code of a base32 secret at a time step. `None` for a secret that does not decode.
pub fn code_at(secret: &str, step: u64) -> Option<String> {
    Some(stored(secret)?.generate(step * STEP_SECS).to_string())
}

/// The step `code` is valid for at `unix` (now, or one step either side), if it is newer than
/// `last_step`. A code is accepted once (RFC 6238 section 5.2), which totp-rs leaves to its caller.
pub fn matching_step(secret: &str, code: &str, unix: u64, last_step: Option<i64>) -> Option<u64> {
    let step = stored(secret)?.check(code, unix)?;
    last_step
        .is_none_or(|last| step as i64 > last)
        .then_some(step)
}

/// A new secret, base32.
pub fn new_secret() -> String {
    Secret::from(crate::random::bytes::<SECRET_BYTES>().to_vec()).to_base32()
}

/// The `otpauth://` URI an authenticator app reads from the QR code.
pub fn otpauth_uri(secret: &str, account: &str) -> Result<String, ApiError> {
    let bytes = Secret::try_from_base32(secret)
        .map_err(|_| ApiError::internal("Stored secret is not base32"))?
        .as_bytes()
        .to_vec();
    generator(bytes, account)?
        .to_url()
        .map_err(|_| ApiError::bad_request("That account cannot be used in an authenticator app"))
}

/// A new recovery code: 80 random bits as sixteen base32 characters, `abcd-efgh-ijkl-mnop`.
pub fn new_recovery_code() -> String {
    let text = Secret::from(crate::random::bytes::<10>().to_vec())
        .to_base32()
        .to_lowercase();
    text.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).expect("ascii"))
        .collect::<Vec<_>>()
        .join("-")
}

/// The stored form of a recovery code: hex SHA-256 of it without dashes or spaces, lowercase.
pub fn hash_recovery_code(code: &str) -> String {
    let normal: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_lowercase();
    hex(&Sha256::digest(normal.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What someone typed: six digits are a code from the app, anything else is tried as a recovery code.
fn is_app_code(input: &str) -> bool {
    input.len() == DIGITS as usize && input.bytes().all(|b| b.is_ascii_digit())
}

/// How a second factor was proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof {
    App,
    RecoveryCode,
}

/// The user's secret once two-factor is on.
pub async fn enabled(db: &impl Db, user_id: uuid::Uuid) -> Result<Option<UserTotp>, ApiError> {
    Ok(user_totp::Entity::find_by_id(user_id)
        .filter(user_totp::Column::EnabledAt.is_not_null())
        .one(db)
        .await?)
}

/// The users among `ids` with two-factor on.
pub async fn enabled_users(
    db: &impl Db,
    ids: impl IntoIterator<Item = uuid::Uuid>,
) -> Result<HashSet<uuid::Uuid>, ApiError> {
    let ids: Vec<_> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(Default::default());
    }
    Ok(user_totp::Entity::find()
        .filter(user_totp::Column::UserId.is_in(ids))
        .filter(user_totp::Column::EnabledAt.is_not_null())
        .all(db)
        .await?
        .into_iter()
        .map(|t| t.user_id)
        .collect())
}

/// Accept `code` for `row` if it matches a step not used yet, recording the step. The update is
/// conditional on the step still being newer, so two requests with one code cannot both pass.
pub async fn accept_app_code(db: &impl Db, row: &UserTotp, code: &str) -> Result<bool, ApiError> {
    let Some(step) = matching_step(&row.secret, code, now_unix(), row.last_step) else {
        return Ok(false);
    };
    let step = step as i64;
    let updated = user_totp::Entity::update_many()
        .col_expr(user_totp::Column::LastStep, Expr::value(step))
        .filter(user_totp::Column::UserId.eq(row.user_id))
        .filter(
            Condition::any()
                .add(user_totp::Column::LastStep.is_null())
                .add(user_totp::Column::LastStep.lt(step)),
        )
        .exec(db)
        .await?
        .rows_affected;
    Ok(updated == 1)
}

/// Check a second factor of someone with two-factor on: a code from the app or an unused recovery code
/// (which is then spent). Call it inside the writer's transaction. `None` when it does not match.
pub async fn verify(db: &impl Db, row: &UserTotp, input: &str) -> Result<Option<Proof>, ApiError> {
    let input = input.trim();
    if is_app_code(input) {
        return Ok(accept_app_code(db, row, input).await?.then_some(Proof::App));
    }
    if input.is_empty() {
        return Ok(None);
    }
    let spent = user_recovery_codes::Entity::update_many()
        .col_expr(
            user_recovery_codes::Column::UsedAt,
            Expr::value(time::now()),
        )
        .filter(user_recovery_codes::Column::UserId.eq(row.user_id))
        .filter(user_recovery_codes::Column::CodeHash.eq(hash_recovery_code(input)))
        .filter(user_recovery_codes::Column::UsedAt.is_null())
        .exec(db)
        .await?
        .rows_affected;
    Ok((spent == 1).then_some(Proof::RecoveryCode))
}

/// Replace the user's recovery codes with new ones, and return them (the only time they are seen).
pub async fn new_recovery_codes(
    db: &impl Db,
    user_id: uuid::Uuid,
) -> Result<Vec<String>, ApiError> {
    user_recovery_codes::Entity::delete_many()
        .filter(user_recovery_codes::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    let now = time::now();
    let codes: Vec<String> = (0..RECOVERY_CODES).map(|_| new_recovery_code()).collect();
    user_recovery_codes::Entity::insert_many(codes.iter().map(|code| {
        user_recovery_codes::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            user_id: Set(user_id),
            code_hash: Set(hash_recovery_code(code)),
            used_at: Set(None),
            created_at: Set(now),
        }
    }))
    .exec(db)
    .await?;
    Ok(codes)
}

/// Turn two-factor off for a user: the secret and the recovery codes go.
pub async fn remove(db: &impl Db, user_id: uuid::Uuid) -> Result<bool, ApiError> {
    let removed = user_totp::Entity::delete_many()
        .filter(user_totp::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;
    user_recovery_codes::Entity::delete_many()
        .filter(user_recovery_codes::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    Ok(removed > 0)
}

/// What a sign-in still needs after its first factor (the password, or the provider).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChallengeMode {
    /// A code from the app (or a recovery code): `POST /auth/login/totp`.
    Code,
    /// Setting two-factor up first, because the site requires it (`TOTP_REQUIRED`):
    /// `POST /auth/login/totp/setup`, then `/enable`.
    Enroll,
}

impl ChallengeMode {
    /// How long the step may take: a code is quick; setting up an app takes longer.
    fn ttl_secs(self) -> i64 {
        match self {
            Self::Code => CHALLENGE_TTL_SECS,
            Self::Enroll => ENROLL_TTL_SECS,
        }
    }
}

/// How long setting up two-factor at sign-in may take.
pub const ENROLL_TTL_SECS: i64 = 15 * 60;

/// What a sign-in of `user_id` needs before it gets a session: a code if two-factor is on, setting it up if
/// the site requires it, else nothing.
pub async fn second_step(
    db: &impl Db,
    required: bool,
    user_id: uuid::Uuid,
) -> Result<Option<ChallengeMode>, ApiError> {
    Ok(if enabled(db, user_id).await?.is_some() {
        Some(ChallengeMode::Code)
    } else if required {
        Some(ChallengeMode::Enroll)
    } else {
        None
    })
}

/// A sign-in waiting for its second step, carried in a signed cookie from the first factor (the password,
/// or the single sign-on callback) to the code.
#[derive(Serialize, Deserialize)]
pub struct Challenge {
    pub user_id: uuid::Uuid,
    /// Unix seconds after which it no longer counts.
    pub expires: i64,
    pub mode: ChallengeMode,
}

impl Challenge {
    pub fn new(user_id: uuid::Uuid, mode: ChallengeMode) -> Self {
        Self {
            user_id,
            expires: jiff::Timestamp::now().as_second() + mode.ttl_secs(),
            mode,
        }
    }

    pub fn is_current(&self) -> bool {
        jiff::Timestamp::now().as_second() < self.expires
    }
}

/// The challenge cookie's name; `__Host-` prefixed over https, like the session cookie.
pub fn challenge_cookie_name(secure: bool) -> &'static str {
    if secure {
        "__Host-woodmouse_2fa"
    } else {
        "woodmouse_2fa"
    }
}

/// The challenge cookie (its value is signed by the caller's jar). `SameSite=Lax`, not Strict: after
/// single sign-on it is set on the way back from the provider, a cross-site navigation.
pub fn challenge_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(challenge_cookie_name(secure), value);
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_max_age(::time::Duration::seconds(ENROLL_TTL_SECS));
    if secure {
        cookie.set_secure(true);
    }
    cookie
}

/// Its removal, with the same attributes (a browser ignores a `__Host-` removal without `Secure`).
pub fn challenge_cookie_removal(secure: bool) -> Cookie<'static> {
    let mut cookie = challenge_cookie(String::new(), secure);
    cookie.make_removal();
    cookie
}

/// The signed challenge cookie for `user_id` in `mode`.
pub fn challenge_jar(
    key: &Key,
    secure: bool,
    user_id: uuid::Uuid,
    mode: ChallengeMode,
) -> Result<SignedCookieJar, ApiError> {
    let value = serde_json::to_string(&Challenge::new(user_id, mode))
        .map_err(|_| ApiError::internal("Could not make the challenge"))?;
    Ok(SignedCookieJar::new(key.clone()).add(challenge_cookie(value, secure)))
}

/// The request's current challenge in `mode`, if this server signed it. A cookie whose signature does not
/// verify (forged, or from before a restart) is not there at all.
pub fn read_challenge(
    headers: &axum::http::HeaderMap,
    key: &Key,
    secure: bool,
    mode: ChallengeMode,
) -> Option<Challenge> {
    SignedCookieJar::from_headers(headers, key.clone())
        .get(challenge_cookie_name(secure))
        .and_then(|c| serde_json::from_str::<Challenge>(c.value()).ok())
        .filter(|c| c.mode == mode && c.is_current())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA-1: the key is the ASCII string `12345678901234567890`, eight digits.
    #[test]
    fn matches_the_rfc_6238_test_vectors() {
        let eight = Builder::new()
            .with_algorithm(Algorithm::SHA1)
            .with_digits(8)
            .with_step_duration(STEP_SECS)
            .with_secret(b"12345678901234567890".to_vec())
            .build()
            .unwrap();
        for (time, expected) in [
            (59, "94287082"),
            (1111111109, "07081804"),
            (1111111111, "14050471"),
            (1234567890, "89005924"),
            (2000000000, "69279037"),
            (20000000000, "65353130"),
        ] {
            assert_eq!(eight.generate(time).to_string(), expected, "at {time}");
        }
        // The same through the six-digit, base32 path the app uses.
        let secret = Secret::from(b"12345678901234567890".to_vec()).to_base32();
        assert_eq!(secret, "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(code_at(&secret, 59 / STEP_SECS).unwrap(), "287082");
        assert_eq!(code_at(&secret, 1111111109 / STEP_SECS).unwrap(), "081804");
    }

    #[test]
    fn codes_work_one_step_either_side_and_once() {
        let secret = Secret::from(b"12345678901234567890".to_vec()).to_base32();
        let now = 1000u64;
        let unix = now * STEP_SECS + 7;
        let code = |step| code_at(&secret, step).unwrap();
        let used = now as i64;
        assert_eq!(matching_step(&secret, &code(now), unix, None), Some(now));
        assert_eq!(
            matching_step(&secret, &code(now - 1), unix, None),
            Some(now - 1)
        );
        assert_eq!(
            matching_step(&secret, &code(now + 1), unix, None),
            Some(now + 1)
        );
        assert_eq!(matching_step(&secret, &code(now - 2), unix, None), None);
        assert_eq!(matching_step(&secret, &code(now + 2), unix, None), None);
        // Used steps, and older ones, are refused.
        assert_eq!(matching_step(&secret, &code(now), unix, Some(used)), None);
        assert_eq!(
            matching_step(&secret, &code(now - 1), unix, Some(used)),
            None
        );
        assert_eq!(
            matching_step(&secret, &code(now + 1), unix, Some(used)),
            Some(now + 1)
        );
    }

    #[test]
    fn secrets_are_random_and_160_bits() {
        let secret = new_secret();
        assert_eq!(
            Secret::try_from_base32(&secret).unwrap().as_bytes().len(),
            20
        );
        assert_ne!(secret, new_secret());
    }

    #[test]
    fn recovery_codes_hash_without_formatting() {
        let code = new_recovery_code();
        assert_eq!(code.len(), 19);
        assert_eq!(
            hash_recovery_code(&code),
            hash_recovery_code(&code.replace('-', "").to_uppercase())
        );
        assert_ne!(
            hash_recovery_code(&code),
            hash_recovery_code(&new_recovery_code())
        );
    }

    #[test]
    fn the_uri_names_the_issuer_and_account() {
        let secret = new_secret();
        let uri = otpauth_uri(&secret, "a+b@home.io").unwrap();
        assert!(
            uri.starts_with("otpauth://totp/Woodmouse:a%2Bb%40home.io?"),
            "{uri}"
        );
        assert!(uri.contains(&format!("secret={secret}")), "{uri}");
        assert!(uri.contains("issuer=Woodmouse"), "{uri}");
    }
}
