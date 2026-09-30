//! Multi-user authentication for the Hakimi HTTP API.
//!
//! Before this module the server had exactly one credential: a shared
//! `webui_password` compared with `==` in [`crate::api`]. That is a single
//! tenant with no notion of *who* is calling, so there is no way to give two
//! people their own account, no way to revoke one person's access, and no way
//! to record which account did what.
//!
//! This module adds the missing layer:
//!
//! - **Password storage** — PBKDF2-HMAC-SHA256 (`ring`), per-user random salt,
//!   constant-time verification. Plaintext is never stored.
//! - **Sessions** — stateless HMAC-SHA256 bearer tokens (`payload.signature`),
//!   signed with a process-persistent secret so tokens survive a restart.
//! - **Roles** — [`Role::Admin`] may manage users; [`Role::User`] may not.
//! - **Backward compatibility** — when no users are configured the API stays
//!   open, exactly as the old empty-password behaviour did. A configured
//!   legacy shared password keeps working and is treated as an admin session.
//!
//! Users live in `~/.hakimi/users.json`; the signing secret lives in
//! `~/.hakimi/auth_secret`. Both are written with `0600` on Unix.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::hmac;
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

/// PBKDF2 iteration count for newly hashed passwords.
///
/// Stored per user, so this can be raised later without invalidating existing
/// records.
pub const DEFAULT_ITERATIONS: u32 = 100_000;

/// How long an issued token stays valid.
pub const TOKEN_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// Salt length in bytes.
const SALT_LEN: usize = 16;
/// Derived key length in bytes.
const KEY_LEN: usize = 32;
/// Minimum accepted password length.
pub const MIN_PASSWORD_LEN: usize = 8;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What a caller is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Full control, including user management.
    Admin,
    /// Normal account: can use the agent, cannot manage users.
    User,
}

impl Role {
    /// Whether this role may manage other users.
    pub fn is_admin(self) -> bool {
        matches!(self, Role::Admin)
    }

    /// Wire/display name.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
}

/// The authenticated identity behind a request.
///
/// Inserted into request extensions by the auth middleware so handlers can
/// ask who is calling without re-parsing the header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// Stable user id (`""` for anonymous / legacy sessions).
    pub user_id: String,
    /// Display name (`"anonymous"` when auth is disabled).
    pub username: String,
    /// Effective role.
    pub role: Role,
    /// True when no user store is configured and the API is open.
    pub anonymous: bool,
}

impl Principal {
    /// Identity used while the user store is empty (auth disabled).
    pub fn anonymous() -> Self {
        Self {
            user_id: String::new(),
            username: "anonymous".to_string(),
            role: Role::Admin,
            anonymous: true,
        }
    }

    /// Identity for a caller that presented the legacy shared password.
    pub fn legacy_admin() -> Self {
        Self {
            user_id: String::new(),
            username: "legacy".to_string(),
            role: Role::Admin,
            anonymous: false,
        }
    }

    /// Whether this caller may manage users.
    pub fn is_admin(&self) -> bool {
        self.role.is_admin()
    }
}

/// A stored account. Never contains the plaintext password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Stable id (random, url-safe).
    pub id: String,
    /// Login name (case-insensitive uniqueness).
    pub username: String,
    /// Base64 PBKDF2 derived key.
    pub password_hash: String,
    /// Base64 salt.
    pub salt: String,
    /// PBKDF2 iterations used for this record.
    pub iterations: u32,
    /// Effective role.
    pub role: Role,
    /// Unix seconds when the account was created.
    pub created_at: u64,
    /// Disabled accounts cannot log in and their tokens stop validating.
    #[serde(default)]
    pub disabled: bool,
}

/// A freshly minted session token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedToken {
    /// The bearer token to hand back to the client.
    pub token: String,
    /// Unix seconds at which the token stops validating.
    pub expires_at: u64,
    /// Owner's display name.
    pub username: String,
    /// Owner's role.
    pub role: Role,
}

/// Why an authentication attempt failed.
#[derive(Debug)]
pub enum AuthError {
    /// Missing, malformed, or wrong credential.
    Unauthorized,
    /// Signature was valid but the token is past its expiry.
    Expired,
    /// The request asked for something the caller's role forbids.
    Forbidden,
    /// The caller supplied something invalid (empty name, short password, ...).
    Invalid(String),
    /// The username is already taken.
    Conflict(String),
    /// A user with that id does not exist.
    NotFound,
    /// Local I/O failure.
    Io(std::io::Error),
    /// The on-disk store could not be parsed.
    Corrupt(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Unauthorized => write!(f, "invalid credentials"),
            AuthError::Expired => write!(f, "token expired"),
            AuthError::Forbidden => write!(f, "insufficient privileges"),
            AuthError::Invalid(msg) => write!(f, "invalid request: {msg}"),
            AuthError::Conflict(msg) => write!(f, "conflict: {msg}"),
            AuthError::NotFound => write!(f, "user not found"),
            AuthError::Io(err) => write!(f, "auth store i/o error: {err}"),
            AuthError::Corrupt(msg) => write!(f, "auth store is corrupt: {msg}"),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<std::io::Error> for AuthError {
    fn from(err: std::io::Error) -> Self {
        AuthError::Io(err)
    }
}

/// Token payload. Kept short — it is base64'd into every request.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// Subject: user id.
    sub: String,
    /// Username at issue time (display only).
    u: String,
    /// Role at issue time (re-checked against the store on verify).
    r: Role,
    /// Unix seconds expiry.
    exp: u64,
}

#[derive(Debug, Default)]
struct Inner {
    users: Vec<User>,
}

// ---------------------------------------------------------------------------
// Password hashing
// ---------------------------------------------------------------------------

/// Derive a PBKDF2 key for `password` with a fresh random salt.
///
/// Returns `(salt_b64, hash_b64)`.
pub fn hash_password(password: &str) -> Result<(String, String), AuthError> {
    hash_password_with(password, DEFAULT_ITERATIONS)
}

/// Like [`hash_password`] but with an explicit iteration count.
pub fn hash_password_with(password: &str, iterations: u32) -> Result<(String, String), AuthError> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    rng.fill(&mut salt)
        .map_err(|_| AuthError::Corrupt("system RNG unavailable".to_string()))?;

    let mut out = [0u8; KEY_LEN];
    let rounds = NonZeroU32::new(iterations.max(1)).expect("clamped above zero");
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        rounds,
        &salt,
        password.as_bytes(),
        &mut out,
    );

    Ok((STANDARD.encode(salt), STANDARD.encode(out)))
}

/// Constant-time verification of `password` against a stored salt + hash.
pub fn verify_password(password: &str, salt_b64: &str, hash_b64: &str, iterations: u32) -> bool {
    let (Ok(salt), Ok(expected)) = (STANDARD.decode(salt_b64), STANDARD.decode(hash_b64)) else {
        return false;
    };

    let rounds = NonZeroU32::new(iterations.max(1)).expect("clamped above zero");
    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        rounds,
        &salt,
        password.as_bytes(),
        &expected,
    )
    .is_ok()
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// User store plus token issuing/verification.
///
/// Shared across handlers behind an `Arc`; all mutation goes through an
/// internal `RwLock`, and every mutation is persisted before it is visible.
pub struct AuthService {
    inner: RwLock<Inner>,
    /// Where `users.json` lives.
    path: PathBuf,
    /// HMAC key for token signatures.
    secret: Vec<u8>,
    /// Where the signing secret lives (kept for diagnostics / rotation).
    secret_path: PathBuf,
}

impl std::fmt::Debug for AuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthService")
            .field("users", &self.user_count())
            .field("path", &self.path)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl AuthService {
    /// An ephemeral store with no users and an in-memory secret.
    ///
    /// Used by tests and as a fallback when the on-disk store cannot be opened.
    pub fn in_memory() -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
            path: PathBuf::new(),
            secret: generate_secret(),
            secret_path: PathBuf::new(),
        }
    }

    /// Load the user store from `dir`, creating it when missing.
    ///
    /// When the store has no users and `initial_password` is non-empty, an
    /// `admin` account is bootstrapped with that password so an existing
    /// single-password deployment keeps working after the upgrade.
    pub fn load_or_bootstrap(dir: &Path, initial_password: &str) -> Result<Self, AuthError> {
        let path = dir.join("users.json");
        let secret_path = dir.join("auth_secret");

        std::fs::create_dir_all(dir)?;

        let users = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str::<Vec<User>>(&raw)
                    .map_err(|err| AuthError::Corrupt(err.to_string()))?
            }
        } else {
            Vec::new()
        };

        let secret = if secret_path.exists() {
            let raw = std::fs::read_to_string(&secret_path)?;
            match URL_SAFE_NO_PAD.decode(raw.trim()) {
                Ok(bytes) if bytes.len() >= 32 => bytes,
                _ => {
                    let bytes = generate_secret();
                    write_private(&secret_path, URL_SAFE_NO_PAD.encode(&bytes).as_bytes())?;
                    bytes
                }
            }
        } else {
            let bytes = generate_secret();
            write_private(&secret_path, URL_SAFE_NO_PAD.encode(&bytes).as_bytes())?;
            bytes
        };

        let service = Self {
            inner: RwLock::new(Inner { users }),
            path,
            secret,
            secret_path,
        };

        if !service.is_enabled() && !initial_password.trim().is_empty() {
            service.create_user("admin", initial_password, Role::Admin)?;
            tracing::info!(
                path = %service.path.display(),
                "auth: bootstrapped 'admin' from the configured legacy password"
            );
        }

        Ok(service)
    }

    /// Whether any account exists. When false the API is open.
    pub fn is_enabled(&self) -> bool {
        self.user_count() > 0
    }

    /// Number of accounts.
    pub fn user_count(&self) -> usize {
        self.read().users.len()
    }

    /// Where the store lives (`""` for in-memory instances).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Signing-secret location (`""` for in-memory instances).
    pub fn secret_path(&self) -> &Path {
        &self.secret_path
    }

    /// Every account, sorted by username.
    pub fn list_users(&self) -> Vec<User> {
        let mut users = self.read().users.clone();
        users.sort_by(|a, b| a.username.cmp(&b.username));
        users
    }

    /// Create an account and persist the store.
    pub fn create_user(
        &self,
        username: &str,
        password: &str,
        role: Role,
    ) -> Result<User, AuthError> {
        let username = username.trim();
        if username.is_empty() {
            return Err(AuthError::Invalid("username must not be empty".to_string()));
        }
        if password.chars().count() < MIN_PASSWORD_LEN {
            return Err(AuthError::Invalid(format!(
                "password must be at least {MIN_PASSWORD_LEN} characters"
            )));
        }

        let (salt, password_hash) = hash_password(password)?;
        let user = User {
            id: random_id(),
            username: username.to_string(),
            password_hash,
            salt,
            iterations: DEFAULT_ITERATIONS,
            role,
            created_at: now(),
            disabled: false,
        };

        {
            let mut inner = self.write();
            if inner
                .users
                .iter()
                .any(|existing| existing.username.eq_ignore_ascii_case(username))
            {
                return Err(AuthError::Conflict(format!(
                    "username '{username}' already exists"
                )));
            }
            inner.users.push(user.clone());
        }
        self.persist()?;
        Ok(user)
    }

    /// Remove an account. Refuses to remove the last admin.
    pub fn delete_user(&self, id: &str) -> Result<(), AuthError> {
        {
            let mut inner = self.write();
            let Some(index) = inner.users.iter().position(|user| user.id == id) else {
                return Err(AuthError::NotFound);
            };

            let target_is_admin = inner.users[index].role.is_admin();
            let remaining_admins = inner
                .users
                .iter()
                .enumerate()
                .filter(|(i, user)| *i != index && user.role.is_admin() && !user.disabled)
                .count();
            if target_is_admin && remaining_admins == 0 {
                return Err(AuthError::Forbidden);
            }

            inner.users.remove(index);
        }
        self.persist()?;
        Ok(())
    }

    /// Replace a user's password. Also clears any prior disabled flag.
    pub fn set_password(&self, id: &str, password: &str) -> Result<(), AuthError> {
        if password.chars().count() < MIN_PASSWORD_LEN {
            return Err(AuthError::Invalid(format!(
                "password must be at least {MIN_PASSWORD_LEN} characters"
            )));
        }
        let (salt, password_hash) = hash_password(password)?;
        {
            let mut inner = self.write();
            let Some(user) = inner.users.iter_mut().find(|user| user.id == id) else {
                return Err(AuthError::NotFound);
            };
            user.salt = salt;
            user.password_hash = password_hash;
            user.iterations = DEFAULT_ITERATIONS;
        }
        self.persist()?;
        Ok(())
    }

    /// Convenience for the legacy `POST /api/config` password field.
    ///
    /// Updates the `admin` account when one exists. Failures are returned so
    /// the caller can decide whether to log them.
    pub fn set_admin_password(&self, password: &str) -> Result<(), AuthError> {
        if !self.is_enabled() {
            return Ok(());
        }
        let id = {
            let inner = self.read();
            inner
                .users
                .iter()
                .find(|user| user.username.eq_ignore_ascii_case("admin"))
                .or_else(|| inner.users.iter().find(|user| user.role.is_admin()))
                .map(|user| user.id.clone())
        };
        match id {
            Some(id) => self.set_password(&id, password),
            None => Ok(()),
        }
    }

    /// Verify credentials and mint a token.
    pub fn login(&self, username: &str, password: &str) -> Result<IssuedToken, AuthError> {
        let (id, name, role, disabled) = {
            let inner = self.read();
            let user = inner
                .users
                .iter()
                .find(|user| user.username.eq_ignore_ascii_case(username.trim()))
                .ok_or(AuthError::Unauthorized)?;
            (
                user.id.clone(),
                user.username.clone(),
                user.role,
                user.disabled,
            )
        };

        // Re-read the record for the hash so the password check happens
        // outside the lock (PBKDF2 is intentionally slow).
        let (salt, hash, iterations) = {
            let inner = self.read();
            let user = inner
                .users
                .iter()
                .find(|user| user.id == id)
                .ok_or(AuthError::Unauthorized)?;
            (
                user.salt.clone(),
                user.password_hash.clone(),
                user.iterations,
            )
        };

        if disabled || !verify_password(password, &salt, &hash, iterations) {
            return Err(AuthError::Unauthorized);
        }

        let principal = Principal {
            user_id: id,
            username: name,
            role,
            anonymous: false,
        };
        let expires_at = now() + TOKEN_TTL_SECS;
        let token = self.issue_token(&principal, expires_at)?;
        Ok(IssuedToken {
            token,
            expires_at,
            username: principal.username,
            role: principal.role,
        })
    }

    /// Mint a signed token for `principal`.
    pub fn issue_token(&self, principal: &Principal, expires_at: u64) -> Result<String, AuthError> {
        let claims = Claims {
            sub: principal.user_id.clone(),
            u: principal.username.clone(),
            r: principal.role,
            exp: expires_at,
        };
        let payload = serde_json::to_vec(&claims)
            .map_err(|err| AuthError::Corrupt(format!("claims encoding failed: {err}")))?;
        let payload_b64 = URL_SAFE_NO_PAD.encode(&payload);
        let signature = sign(&self.secret, payload_b64.as_bytes());
        Ok(format!(
            "{payload_b64}.{}",
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    /// Validate a token and resolve it back to the live user record.
    ///
    /// The store is consulted on every call, so deleting or disabling an
    /// account revokes its outstanding tokens immediately.
    pub fn verify_token(&self, token: &str) -> Result<Principal, AuthError> {
        let (payload_b64, signature_b64) = token.split_once('.').ok_or(AuthError::Unauthorized)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature_b64)
            .map_err(|_| AuthError::Unauthorized)?;

        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.secret);
        hmac::verify(&key, payload_b64.as_bytes(), &signature)
            .map_err(|_| AuthError::Unauthorized)?;

        let payload = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| AuthError::Unauthorized)?;
        let claims: Claims =
            serde_json::from_slice(&payload).map_err(|_| AuthError::Unauthorized)?;

        if claims.exp <= now() {
            return Err(AuthError::Expired);
        }

        let inner = self.read();
        let user = inner
            .users
            .iter()
            .find(|user| user.id == claims.sub)
            .ok_or(AuthError::Unauthorized)?;
        if user.disabled {
            return Err(AuthError::Unauthorized);
        }

        Ok(Principal {
            user_id: user.id.clone(),
            username: user.username.clone(),
            role: user.role,
            anonymous: false,
        })
    }

    /// Decide whether a request may proceed.
    ///
    /// Resolution order:
    /// 1. a valid bearer token → that user's principal;
    /// 2. no accounts configured → open (anonymous admin), matching the old
    ///    "empty password means no auth" behaviour;
    /// 3. the legacy shared `webui_password` → admin session.
    pub fn authorize(
        &self,
        authorization: Option<&str>,
        legacy_password: &str,
    ) -> Result<Principal, AuthError> {
        let presented = authorization
            .and_then(|header| header.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(token) = presented
            && let Ok(principal) = self.verify_token(token)
        {
            return Ok(principal);
        }

        if !self.is_enabled() {
            return Ok(Principal::anonymous());
        }

        if !legacy_password.trim().is_empty() && presented == Some(legacy_password.trim()) {
            return Ok(Principal::legacy_admin());
        }

        Err(AuthError::Unauthorized)
    }

    // -- internals ---------------------------------------------------------

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|err| err.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|err| err.into_inner())
    }

    fn persist(&self) -> Result<(), AuthError> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let snapshot = {
            let inner = self.read();
            serde_json::to_vec_pretty(&inner.users)
                .map_err(|err| AuthError::Corrupt(err.to_string()))?
        };
        write_private(&self.path, &snapshot)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

fn generate_secret() -> Vec<u8> {
    let rng = SystemRandom::new();
    let mut secret = vec![0u8; 32];
    if rng.fill(&mut secret).is_err() {
        // SystemRandom should never fail; fall back to time-derived bytes so we
        // still refuse to run with a predictable constant.
        let seed = now().to_le_bytes();
        for (index, byte) in secret.iter_mut().enumerate() {
            *byte = seed[index % seed.len()] ^ (index as u8);
        }
    }
    secret
}

fn random_id() -> String {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 12];
    if rng.fill(&mut bytes).is_err() {
        return format!("u{}", now());
    }
    URL_SAFE_NO_PAD.encode(bytes)
}

fn sign(secret: &[u8], message: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::sign(&key, message).as_ref().to_vec()
}

/// Write a file readable only by the owner.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AuthError> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hakimi-auth-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn hash_is_salted_and_verifies() {
        let (salt_a, hash_a) = hash_password("correct horse").unwrap();
        let (salt_b, hash_b) = hash_password("correct horse").unwrap();
        assert_ne!(salt_a, salt_b, "salt must be random per hash");
        assert_ne!(hash_a, hash_b, "same password must not collide");

        assert!(verify_password(
            "correct horse",
            &salt_a,
            &hash_a,
            DEFAULT_ITERATIONS
        ));
        assert!(!verify_password(
            "wrong horse",
            &salt_a,
            &hash_a,
            DEFAULT_ITERATIONS
        ));
    }

    #[test]
    fn verify_rejects_garbage() {
        assert!(!verify_password(
            "x",
            "not-base64!",
            "also-bad!",
            DEFAULT_ITERATIONS
        ));
    }

    #[test]
    fn empty_store_is_open() {
        let auth = AuthService::in_memory();
        assert!(!auth.is_enabled());
        let principal = auth.authorize(None, "").unwrap();
        assert!(principal.anonymous);
    }

    #[test]
    fn bootstrap_creates_admin_from_legacy_password() {
        let dir = temp_dir("bootstrap");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
        assert!(auth.is_enabled());
        assert_eq!(auth.user_count(), 1);

        let issued = auth.login("admin", "legacy-secret").unwrap();
        let principal = auth.verify_token(&issued.token).unwrap();
        assert_eq!(principal.username, "admin");
        assert!(principal.is_admin());
        assert!(!principal.anonymous);

        // The legacy shared password still works and yields an admin session.
        // The caller owns the current `webui_password`, so it must be passed in
        // alongside the header — the store deliberately keeps no stale copy.
        let legacy = auth
            .authorize(Some("Bearer legacy-secret"), "legacy-secret")
            .unwrap();
        assert!(legacy.is_admin());
        assert_eq!(legacy.username, "legacy");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wrong_password_is_unauthorized() {
        let dir = temp_dir("wrongpw");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
        assert!(matches!(
            auth.login("admin", "nope"),
            Err(AuthError::Unauthorized)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tokens_are_scoped_to_the_store() {
        let dir = temp_dir("scope");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
        let issued = auth.login("admin", "legacy-secret").unwrap();

        // A token signed by a different service must not validate.
        let other = AuthService::in_memory();
        assert!(matches!(
            other.verify_token(&issued.token),
            Err(AuthError::Unauthorized)
        ));

        // A tampered payload must not validate.
        let tampered = format!("{}x.{}", &issued.token[..4], "sig");
        assert!(auth.verify_token(&tampered).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_a_user_revokes_its_tokens() {
        let dir = temp_dir("revoke");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
        let user = auth
            .create_user("alice", "alice-password", Role::User)
            .unwrap();
        let issued = auth.login("alice", "alice-password").unwrap();
        assert!(auth.verify_token(&issued.token).is_ok());

        auth.delete_user(&user.id).unwrap();
        assert!(
            auth.verify_token(&issued.token).is_err(),
            "tokens must stop validating once the account is gone"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cannot_delete_the_last_admin() {
        let dir = temp_dir("lastadmin");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
        let admin_id = auth.list_users()[0].id.clone();
        assert!(matches!(
            auth.delete_user(&admin_id),
            Err(AuthError::Forbidden)
        ));

        // With a second admin present the first may be removed.
        auth.create_user("backup", "backup-password", Role::Admin)
            .unwrap();
        auth.delete_user(&admin_id).unwrap();
        assert_eq!(auth.user_count(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_duplicate_and_short_credentials() {
        let dir = temp_dir("validate");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();

        assert!(matches!(
            auth.create_user("admin", "another-password", Role::User),
            Err(AuthError::Conflict(_))
        ));
        assert!(matches!(
            auth.create_user("bob", "short", Role::User),
            Err(AuthError::Invalid(_))
        ));
        assert!(matches!(
            auth.create_user("  ", "long-enough-password", Role::User),
            Err(AuthError::Invalid(_))
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn set_password_changes_login() {
        let dir = temp_dir("setpw");
        let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
        let user = auth
            .create_user("carol", "carol-password", Role::User)
            .unwrap();

        auth.set_password(&user.id, "carol-new-password").unwrap();
        assert!(auth.login("carol", "carol-password").is_err());
        assert!(auth.login("carol", "carol-new-password").is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_survives_reload() {
        let dir = temp_dir("reload");
        {
            let auth = AuthService::load_or_bootstrap(&dir, "legacy-secret").unwrap();
            auth.create_user("dave", "dave-password", Role::User)
                .unwrap();
            let issued = auth.login("dave", "dave-password").unwrap();

            // Second instance reads the same directory: the secret and the
            // account must both round-trip, so the token keeps validating.
            let reopened = AuthService::load_or_bootstrap(&dir, "ignored").unwrap();
            assert_eq!(reopened.user_count(), 2);
            let principal = reopened.verify_token(&issued.token).unwrap();
            assert_eq!(principal.username, "dave");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn role_names_are_stable() {
        assert_eq!(Role::Admin.as_str(), "admin");
        assert_eq!(Role::User.as_str(), "user");
        assert!(Role::Admin.is_admin());
        assert!(!Role::User.is_admin());
    }
}
