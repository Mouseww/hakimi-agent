//! Credential Pool — manages multiple API keys per provider with
//! configurable rotation strategies and automatic exhaustion detection.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Instant helpers – Instant doesn't implement Serialize/Deserialize, so we
// store timing data as optional u64 millis and provide helper conversions.
// ---------------------------------------------------------------------------

/// Monotonic-ish millisecond counter anchored to process start.
fn now_millis() -> u64 {
    use std::time::Instant;
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}

/// Error threshold before auto-exhausting a credential.
const ERROR_THRESHOLD: u32 = 5;

// ---------------------------------------------------------------------------
// Credential
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    /// Unique identifier for this credential.
    pub id: String,
    /// API key value.
    pub api_key: String,
    /// Provider-specific base URL override.
    pub base_url: Option<String>,
    /// Organization ID (some providers require this).
    pub org_id: Option<String>,
    /// Higher values are preferred during selection.
    pub priority: i32,
    /// Maximum concurrent requests allowed for this credential.
    pub max_concurrent: usize,
    /// Number of currently active (in-flight) requests.
    #[serde(default)]
    pub active_requests: usize,
    /// Whether the credential has been marked as exhausted.
    #[serde(default)]
    pub is_exhausted: bool,
    /// If exhausted, the monotonic-millis timestamp when cooldown expires.
    #[serde(default)]
    pub exhausted_until_ms: Option<u64>,
    /// Consecutive error count.
    #[serde(default)]
    pub error_count: u32,
    /// Monotonic-millis timestamp of last use.
    #[serde(default)]
    pub last_used_ms: Option<u64>,
    /// Lifetime total requests made through this credential.
    #[serde(default)]
    pub total_requests: u64,
    /// Lifetime total errors from this credential.
    #[serde(default)]
    pub total_errors: u64,
}

impl Credential {
    /// Convenience constructor with sensible defaults.
    pub fn new(id: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            api_key: api_key.into(),
            base_url: None,
            org_id: None,
            priority: 0,
            max_concurrent: 10,
            active_requests: 0,
            is_exhausted: false,
            exhausted_until_ms: None,
            error_count: 0,
            last_used_ms: None,
            total_requests: 0,
            total_errors: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// RotationStrategy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RotationStrategy {
    /// Cycle through credentials one at a time.
    RoundRobin,
    /// Use the highest-priority credential up to max_concurrent before moving on.
    FillFirst,
    /// Randomly pick from available credentials.
    Random,
    /// Pick the credential with the fewest total requests.
    LeastUsed,
}

impl std::fmt::Display for RotationStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RotationStrategy::RoundRobin => write!(f, "round_robin"),
            RotationStrategy::FillFirst => write!(f, "fill_first"),
            RotationStrategy::Random => write!(f, "random"),
            RotationStrategy::LeastUsed => write!(f, "least_used"),
        }
    }
}

impl std::str::FromStr for RotationStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "round_robin" | "round-robin" | "roundrobin" => Ok(Self::RoundRobin),
            "fill_first" | "fill-first" | "fillfirst" => Ok(Self::FillFirst),
            "random" => Ok(Self::Random),
            "least_used" | "least-used" | "leastused" => Ok(Self::LeastUsed),
            _ => Err(format!("unknown rotation strategy: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// PoolStats / CredentialStats
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct CredentialStats {
    pub id: String,
    pub requests: u64,
    pub errors: u64,
    pub error_rate: f64,
    pub is_exhausted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PoolStats {
    pub total: usize,
    pub available: usize,
    pub exhausted: usize,
    pub total_requests: u64,
    pub total_errors: u64,
    pub per_credential: Vec<CredentialStats>,
}

// ---------------------------------------------------------------------------
// CredentialPool
// ---------------------------------------------------------------------------

pub struct CredentialPool {
    pub provider_name: String,
    pub credentials: Vec<Credential>,
    pub strategy: RotationStrategy,
    /// Index used by the round-robin strategy.
    current_index: usize,
}

impl CredentialPool {
    /// Create a new empty credential pool.
    pub fn new(provider_name: &str, strategy: RotationStrategy) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            credentials: Vec::new(),
            strategy,
            current_index: 0,
        }
    }

    /// Add a credential to the pool.
    pub fn add_credential(&mut self, cred: Credential) {
        self.credentials.push(cred);
    }

    /// Remove a credential by id. Returns true if found and removed.
    pub fn remove_credential(&mut self, id: &str) -> bool {
        let len_before = self.credentials.len();
        self.credentials.retain(|c| c.id != id);
        if self.current_index >= self.credentials.len() && !self.credentials.is_empty() {
            self.current_index %= self.credentials.len();
        }
        self.credentials.len() < len_before
    }

    /// Determine if a credential is currently available for use.
    pub fn is_available(cred: &Credential) -> bool {
        if cred.is_exhausted {
            if let Some(until) = cred.exhausted_until_ms {
                if now_millis() < until {
                    return false;
                }
                // Cooldown has expired — treat as available.
            } else {
                // Exhausted with no cooldown expiry => permanently exhausted
                // until explicitly cleared.
                return false;
            }
        }
        cred.active_requests < cred.max_concurrent
    }

    /// Acquire the next available credential according to the rotation strategy.
    ///
    /// Increments `active_requests` and `total_requests` on the returned
    /// credential, and updates `last_used_ms`.
    pub fn acquire(&mut self) -> Option<Credential> {
        self.refresh();

        let available_indices: Vec<usize> = self
            .credentials
            .iter()
            .enumerate()
            .filter(|(_, c)| Self::is_available(c))
            .map(|(i, _)| i)
            .collect();

        if available_indices.is_empty() {
            return None;
        }

        let idx = match self.strategy {
            RotationStrategy::RoundRobin => self.acquire_round_robin(&available_indices),
            RotationStrategy::FillFirst => self.acquire_fill_first(&available_indices),
            RotationStrategy::Random => self.acquire_random(&available_indices),
            RotationStrategy::LeastUsed => self.acquire_least_used(&available_indices),
        };

        let cred = &mut self.credentials[idx];
        cred.active_requests += 1;
        cred.total_requests += 1;
        cred.last_used_ms = Some(now_millis());
        Some(cred.clone())
    }

    /// Round-robin: find the next index in cyclic order.
    fn acquire_round_robin(&mut self, available: &[usize]) -> usize {
        let n = self.credentials.len();
        for offset in 0..n {
            let candidate = (self.current_index + offset) % n;
            if available.contains(&candidate) {
                self.current_index = (candidate + 1) % n;
                return candidate;
            }
        }
        // Fallback
        self.current_index = (available[0] + 1) % n;
        available[0]
    }

    /// Fill-first: pick the highest-priority credential that still has capacity.
    /// If multiple share the same priority, prefer the one with fewer active
    /// requests (and break ties by position).
    fn acquire_fill_first(&self, available: &[usize]) -> usize {
        *available
            .iter()
            .min_by_key(|&&i| {
                let c = &self.credentials[i];
                // Negate priority so higher priority sorts first.
                (-c.priority, c.active_requests as i64, i)
            })
            .unwrap()
    }

    /// Random selection from available.
    fn acquire_random(&self, available: &[usize]) -> usize {
        use rand::Rng;
        let mut rng = rand::rng();
        let pick = rng.random_range(0..available.len());
        available[pick]
    }

    /// Least-used: credential with fewest total requests.
    fn acquire_least_used(&self, available: &[usize]) -> usize {
        *available
            .iter()
            .min_by_key(|&&i| {
                let c = &self.credentials[i];
                (c.total_requests, i)
            })
            .unwrap()
    }

    /// Release a credential (decrement active_requests).
    pub fn release(&mut self, id: &str) {
        if let Some(cred) = self.credentials.iter_mut().find(|c| c.id == id) {
            cred.active_requests = cred.active_requests.saturating_sub(1);
        }
    }

    /// Mark a credential as exhausted for `cooldown_ms` milliseconds.
    pub fn mark_exhausted(&mut self, id: &str, cooldown_ms: u64) {
        if let Some(cred) = self.credentials.iter_mut().find(|c| c.id == id) {
            cred.is_exhausted = true;
            cred.exhausted_until_ms = Some(now_millis() + cooldown_ms);
        }
    }

    /// Record an error for a credential. Auto-exhausts after ERROR_THRESHOLD
    /// consecutive errors.
    pub fn mark_error(&mut self, id: &str) {
        if let Some(cred) = self.credentials.iter_mut().find(|c| c.id == id) {
            cred.error_count += 1;
            cred.total_errors += 1;
            if cred.error_count >= ERROR_THRESHOLD {
                cred.is_exhausted = true;
                cred.exhausted_until_ms = Some(now_millis() + 60_000);
            }
        }
    }

    /// Record a successful request — resets the consecutive error count.
    pub fn mark_success(&mut self, id: &str) {
        if let Some(cred) = self.credentials.iter_mut().find(|c| c.id == id) {
            cred.error_count = 0;
        }
    }

    /// Number of credentials that are currently available.
    pub fn available_count(&self) -> usize {
        self.credentials
            .iter()
            .filter(|c| Self::is_available(c))
            .count()
    }

    /// Total credentials in the pool.
    pub fn total_count(&self) -> usize {
        self.credentials.len()
    }

    /// Clear expired exhaustion markers.
    pub fn refresh(&mut self) {
        let now = now_millis();
        for cred in &mut self.credentials {
            if cred.is_exhausted
                && let Some(until) = cred.exhausted_until_ms
                && now >= until
            {
                cred.is_exhausted = false;
                cred.exhausted_until_ms = None;
                cred.error_count = 0;
            }
        }
    }

    /// Compute pool-level statistics.
    pub fn stats(&self) -> PoolStats {
        let total = self.credentials.len();
        let available = self.available_count();
        let exhausted = self.credentials.iter().filter(|c| c.is_exhausted).count();
        let total_requests: u64 = self.credentials.iter().map(|c| c.total_requests).sum();
        let total_errors: u64 = self.credentials.iter().map(|c| c.total_errors).sum();

        let per_credential = self
            .credentials
            .iter()
            .map(|c| CredentialStats {
                id: c.id.clone(),
                requests: c.total_requests,
                errors: c.total_errors,
                error_rate: if c.total_requests > 0 {
                    c.total_errors as f64 / c.total_requests as f64
                } else {
                    0.0
                },
                is_exhausted: c.is_exhausted,
            })
            .collect();

        PoolStats {
            total,
            available,
            exhausted,
            total_requests,
            total_errors,
            per_credential,
        }
    }

    /// Serialize the pool to a JSON string.
    pub fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct PoolJson<'a> {
            provider_name: &'a str,
            credentials: &'a Vec<Credential>,
            strategy: &'a RotationStrategy,
            current_index: usize,
        }

        let pj = PoolJson {
            provider_name: &self.provider_name,
            credentials: &self.credentials,
            strategy: &self.strategy,
            current_index: self.current_index,
        };
        serde_json::to_string(&pj).expect("serialization should not fail")
    }

    /// Deserialize a pool from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct PoolJson {
            provider_name: String,
            credentials: Vec<Credential>,
            strategy: RotationStrategy,
            #[serde(default)]
            current_index: usize,
        }

        let pj: PoolJson = serde_json::from_str(json)?;
        Ok(Self {
            provider_name: pj.provider_name,
            credentials: pj.credentials,
            strategy: pj.strategy,
            current_index: pj.current_index,
        })
    }
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

/// Per-credential configuration entry (used in config files).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialConfig {
    /// Optional identifier; auto-generated if omitted.
    pub id: Option<String>,
    /// The API key.
    pub api_key: String,
    /// Provider-specific base URL override.
    pub base_url: Option<String>,
    /// Organization ID.
    pub org_id: Option<String>,
    /// Selection priority (higher = preferred).
    pub priority: Option<i32>,
    /// Max concurrent requests for this credential.
    pub max_concurrent: Option<usize>,
}

/// Configuration for a credential pool (one per provider).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialPoolConfig {
    /// Rotation strategy name: "round_robin", "fill_first", "random", "least_used".
    pub strategy: Option<String>,
    /// Credentials in this pool.
    pub credentials: Vec<CredentialConfig>,
}

impl CredentialPoolConfig {
    /// Build a [`CredentialPool`] from this configuration.
    pub fn to_pool(&self, provider_name: &str) -> CredentialPool {
        let strategy = self
            .strategy
            .as_deref()
            .map(|s| {
                s.parse::<RotationStrategy>()
                    .unwrap_or(RotationStrategy::RoundRobin)
            })
            .unwrap_or(RotationStrategy::RoundRobin);

        let mut pool = CredentialPool::new(provider_name, strategy);
        for (i, cc) in self.credentials.iter().enumerate() {
            let id = cc
                .id
                .clone()
                .unwrap_or_else(|| format!("{provider_name}-cred-{i}"));
            let mut cred = Credential::new(id, &cc.api_key);
            cred.base_url = cc.base_url.clone();
            cred.org_id = cc.org_id.clone();
            cred.priority = cc.priority.unwrap_or(0);
            cred.max_concurrent = cc.max_concurrent.unwrap_or(10);
            pool.add_credential(cred);
        }
        pool
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cred(id: &str) -> Credential {
        Credential::new(id, format!("key-{id}"))
    }

    fn make_cred_with_priority(id: &str, priority: i32) -> Credential {
        let mut c = make_cred(id);
        c.priority = priority;
        c
    }

    // --- Pool lifecycle ---

    #[test]
    fn test_create_pool() {
        let pool = CredentialPool::new("openai", RotationStrategy::RoundRobin);
        assert_eq!(pool.provider_name, "openai");
        assert_eq!(pool.total_count(), 0);
        assert_eq!(pool.available_count(), 0);
        assert_eq!(pool.strategy, RotationStrategy::RoundRobin);
    }

    #[test]
    fn test_add_credential() {
        let mut pool = CredentialPool::new("openai", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        assert_eq!(pool.total_count(), 2);
        assert_eq!(pool.available_count(), 2);
    }

    #[test]
    fn test_remove_credential() {
        let mut pool = CredentialPool::new("openai", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        assert!(pool.remove_credential("a"));
        assert_eq!(pool.total_count(), 1);
        assert!(!pool.remove_credential("nonexistent"));
    }

    // --- Acquire strategies ---

    #[test]
    fn test_acquire_round_robin() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.add_credential(make_cred("c"));

        let first = pool.acquire().unwrap();
        let second = pool.acquire().unwrap();
        let third = pool.acquire().unwrap();
        assert_eq!(first.id, "a");
        assert_eq!(second.id, "b");
        assert_eq!(third.id, "c");

        // Release all and acquire again — should wrap around to "a"
        pool.release("a");
        pool.release("b");
        pool.release("c");
        let fourth = pool.acquire().unwrap();
        assert_eq!(fourth.id, "a");
    }

    #[test]
    fn test_acquire_fill_first() {
        let mut pool = CredentialPool::new("p", RotationStrategy::FillFirst);
        let mut a = make_cred_with_priority("a", 10);
        a.max_concurrent = 2;
        let b = make_cred_with_priority("b", 5);
        pool.add_credential(a);
        pool.add_credential(b);

        // Both should go to "a" first (higher priority)
        let r1 = pool.acquire().unwrap();
        assert_eq!(r1.id, "a");
        let r2 = pool.acquire().unwrap();
        assert_eq!(r2.id, "a");
        // "a" is now at max_concurrent, so next should be "b"
        let r3 = pool.acquire().unwrap();
        assert_eq!(r3.id, "b");
    }

    #[test]
    fn test_acquire_random() {
        // Acquire many times from fresh pools; should get at least one of each.
        let mut saw_a = false;
        let mut saw_b = false;
        for _ in 0..100 {
            let mut tmp = CredentialPool::new("p", RotationStrategy::Random);
            tmp.add_credential(make_cred("a"));
            tmp.add_credential(make_cred("b"));
            let r = tmp.acquire().unwrap();
            if r.id == "a" {
                saw_a = true;
            }
            if r.id == "b" {
                saw_b = true;
            }
            if saw_a && saw_b {
                break;
            }
        }
        assert!(saw_a, "should have picked 'a' at least once");
        assert!(saw_b, "should have picked 'b' at least once");
    }

    #[test]
    fn test_acquire_least_used() {
        let mut pool = CredentialPool::new("p", RotationStrategy::LeastUsed);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        // First acquire picks "a" (both at 0, tie-break by index)
        let r1 = pool.acquire().unwrap();
        assert_eq!(r1.id, "a");

        // Now "a" has 1 request, "b" has 0 → next should be "b"
        let r2 = pool.acquire().unwrap();
        assert_eq!(r2.id, "b");
    }

    // --- Exhaustion ---

    #[test]
    fn test_acquire_exhausted_skipped() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        pool.mark_exhausted("a", 60_000);
        let r = pool.acquire().unwrap();
        assert_eq!(r.id, "b");
    }

    #[test]
    fn test_mark_exhausted() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        pool.mark_exhausted("a", 60_000);
        assert!(pool.credentials[0].is_exhausted);
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn test_mark_error_threshold() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        for _ in 0..ERROR_THRESHOLD {
            pool.mark_error("a");
        }
        // Should now be auto-exhausted
        assert!(pool.credentials[0].is_exhausted);
        assert_eq!(pool.available_count(), 0);
    }

    #[test]
    fn test_mark_success_resets() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        pool.mark_error("a");
        pool.mark_error("a");
        assert_eq!(pool.credentials[0].error_count, 2);

        pool.mark_success("a");
        assert_eq!(pool.credentials[0].error_count, 0);
    }

    // --- Release ---

    #[test]
    fn test_release_decrements() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        pool.acquire().unwrap();
        assert_eq!(pool.credentials[0].active_requests, 1);

        pool.release("a");
        assert_eq!(pool.credentials[0].active_requests, 0);
    }

    // --- Refresh ---

    #[test]
    fn test_refresh_clears_expired() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        // Exhaust with 0ms cooldown — it's already expired.
        pool.mark_exhausted("a", 0);
        assert!(pool.credentials[0].is_exhausted);

        // refresh should clear it
        pool.refresh();
        assert!(!pool.credentials[0].is_exhausted);
        assert_eq!(pool.available_count(), 1);
    }

    // --- Counts ---

    #[test]
    fn test_available_count() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.add_credential(make_cred("c"));

        pool.mark_exhausted("b", 60_000);
        assert_eq!(pool.available_count(), 2);
        assert_eq!(pool.total_count(), 3);
    }

    // --- Stats ---

    #[test]
    fn test_stats() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        pool.acquire().unwrap();
        pool.acquire().unwrap();
        pool.mark_error("b");

        let stats = pool.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.available, 2);
        assert_eq!(stats.exhausted, 0);
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.total_errors, 1);

        let a_stats = stats.per_credential.iter().find(|s| s.id == "a").unwrap();
        assert_eq!(a_stats.requests, 1);
        assert_eq!(a_stats.errors, 0);
        assert!((a_stats.error_rate - 0.0).abs() < f64::EPSILON);

        let b_stats = stats.per_credential.iter().find(|s| s.id == "b").unwrap();
        assert_eq!(b_stats.requests, 1);
        assert_eq!(b_stats.errors, 1);
        assert!((b_stats.error_rate - 1.0).abs() < f64::EPSILON);
    }

    // --- Serialization ---

    #[test]
    fn test_serialization_roundtrip() {
        let mut pool = CredentialPool::new("openai", RotationStrategy::FillFirst);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.current_index = 1;

        let json = pool.to_json();
        let restored = CredentialPool::from_json(&json).unwrap();

        assert_eq!(restored.provider_name, "openai");
        assert_eq!(restored.strategy, RotationStrategy::FillFirst);
        assert_eq!(restored.credentials.len(), 2);
        assert_eq!(restored.credentials[0].id, "a");
        assert_eq!(restored.credentials[1].id, "b");
        assert_eq!(restored.current_index, 1);
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_pool_returns_none() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn test_all_exhausted_returns_none() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        pool.mark_exhausted("a", 60_000);
        pool.mark_exhausted("b", 60_000);
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn test_concurrent_limit_respected() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        let mut a = make_cred("a");
        a.max_concurrent = 1;
        let mut b = make_cred("b");
        b.max_concurrent = 1;
        pool.add_credential(a);
        pool.add_credential(b);

        let r1 = pool.acquire().unwrap();
        assert_eq!(r1.id, "a");
        let r2 = pool.acquire().unwrap();
        assert_eq!(r2.id, "b");

        // Both at capacity → None
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn test_priority_ordering() {
        let mut pool = CredentialPool::new("p", RotationStrategy::FillFirst);
        pool.add_credential(make_cred_with_priority("low", 1));
        pool.add_credential(make_cred_with_priority("high", 10));
        pool.add_credential(make_cred_with_priority("mid", 5));

        let r = pool.acquire().unwrap();
        assert_eq!(r.id, "high");
    }

    // --- Config integration ---

    #[test]
    fn test_config_integration() {
        let cfg = CredentialPoolConfig {
            strategy: Some("fill_first".to_string()),
            credentials: vec![
                CredentialConfig {
                    id: Some("key1".to_string()),
                    api_key: "sk-test-1".to_string(),
                    base_url: Some("https://api.example.com".to_string()),
                    org_id: Some("org1".to_string()),
                    priority: Some(10),
                    max_concurrent: Some(5),
                },
                CredentialConfig {
                    id: None,
                    api_key: "sk-test-2".to_string(),
                    base_url: None,
                    org_id: None,
                    priority: None,
                    max_concurrent: None,
                },
            ],
        };

        let pool = cfg.to_pool("openai");
        assert_eq!(pool.provider_name, "openai");
        assert_eq!(pool.strategy, RotationStrategy::FillFirst);
        assert_eq!(pool.total_count(), 2);

        assert_eq!(pool.credentials[0].id, "key1");
        assert_eq!(pool.credentials[0].api_key, "sk-test-1");
        assert_eq!(
            pool.credentials[0].base_url,
            Some("https://api.example.com".to_string())
        );
        assert_eq!(pool.credentials[0].org_id, Some("org1".to_string()));
        assert_eq!(pool.credentials[0].priority, 10);
        assert_eq!(pool.credentials[0].max_concurrent, 5);

        assert_eq!(pool.credentials[1].id, "openai-cred-1");
        assert_eq!(pool.credentials[1].api_key, "sk-test-2");
        assert_eq!(pool.credentials[1].priority, 0);
        assert_eq!(pool.credentials[1].max_concurrent, 10);
    }

    // --- RotationStrategy parsing ---

    #[test]
    fn test_rotation_strategy_from_str() {
        assert_eq!(
            "round_robin".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::RoundRobin
        );
        assert_eq!(
            "fill_first".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::FillFirst
        );
        assert_eq!(
            "random".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::Random
        );
        assert_eq!(
            "least_used".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::LeastUsed
        );
        assert!("unknown".parse::<RotationStrategy>().is_err());
    }

    #[test]
    fn test_release_underflow_saturates() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        // Release without acquire should not panic
        pool.release("a");
        assert_eq!(pool.credentials[0].active_requests, 0);
    }

    #[test]
    fn test_mark_error_unknown_id_noop() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        // Marking error on non-existent id should not panic
        pool.mark_error("nonexistent");
        assert_eq!(pool.credentials[0].error_count, 0);
    }

    #[test]
    fn test_stats_empty_pool() {
        let pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        let stats = pool.stats();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.available, 0);
        assert_eq!(stats.exhausted, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_errors, 0);
        assert!(stats.per_credential.is_empty());
    }

    // --- Additional comprehensive tests ---

    #[test]
    fn test_round_robin_cycles_through_all() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.add_credential(make_cred("c"));

        // Cycle 1
        let r1 = pool.acquire().unwrap();
        let r2 = pool.acquire().unwrap();
        let r3 = pool.acquire().unwrap();
        assert_eq!(r1.id, "a");
        assert_eq!(r2.id, "b");
        assert_eq!(r3.id, "c");

        // Release and cycle 2
        pool.release("a");
        pool.release("b");
        pool.release("c");

        let r4 = pool.acquire().unwrap();
        let r5 = pool.acquire().unwrap();
        let r6 = pool.acquire().unwrap();
        assert_eq!(r4.id, "a");
        assert_eq!(r5.id, "b");
        assert_eq!(r6.id, "c");
    }

    #[test]
    fn test_single_credential_pool() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("only"));

        let r = pool.acquire().unwrap();
        assert_eq!(r.id, "only");
        assert_eq!(pool.total_count(), 1);
        assert_eq!(pool.available_count(), 1); // max_concurrent=10, active_requests=1, still available
    }

    #[test]
    fn test_single_credential_exhausted_returns_none() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("only"));

        pool.mark_exhausted("only", 60_000);
        assert!(pool.acquire().is_none());
    }

    #[test]
    fn test_add_and_remove_multiple_credentials() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.add_credential(make_cred("c"));
        pool.add_credential(make_cred("d"));
        assert_eq!(pool.total_count(), 4);

        assert!(pool.remove_credential("b"));
        assert_eq!(pool.total_count(), 3);
        assert!(pool.remove_credential("d"));
        assert_eq!(pool.total_count(), 2);
        assert!(!pool.remove_credential("b")); // already removed
        assert_eq!(pool.total_count(), 2);
    }

    #[test]
    fn test_acquire_updates_total_requests() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        pool.acquire().unwrap();
        pool.release("a");
        pool.acquire().unwrap();

        assert_eq!(pool.credentials[0].total_requests, 2);
    }

    #[test]
    fn test_acquire_sets_last_used_ms() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        assert!(pool.credentials[0].last_used_ms.is_none());
        pool.acquire().unwrap();
        assert!(pool.credentials[0].last_used_ms.is_some());
    }

    #[test]
    fn test_mark_error_increments_counts() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        pool.mark_error("a");
        pool.mark_error("a");
        assert_eq!(pool.credentials[0].error_count, 2);
        assert_eq!(pool.credentials[0].total_errors, 2);
    }

    #[test]
    fn test_mark_error_below_threshold_not_exhausted() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        for _ in 0..ERROR_THRESHOLD - 1 {
            pool.mark_error("a");
        }
        assert!(!pool.credentials[0].is_exhausted);
        assert_eq!(pool.available_count(), 1);
    }

    #[test]
    fn test_mark_success_resets_error_count_after_errors() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));

        pool.mark_error("a");
        pool.mark_error("a");
        pool.mark_error("a");
        assert_eq!(pool.credentials[0].error_count, 3);
        assert_eq!(pool.credentials[0].total_errors, 3);

        pool.mark_success("a");
        assert_eq!(pool.credentials[0].error_count, 0);
        // total_errors should NOT reset — it's a lifetime counter
        assert_eq!(pool.credentials[0].total_errors, 3);
    }

    #[test]
    fn test_fill_first_respects_priority_ordering() {
        let mut pool = CredentialPool::new("p", RotationStrategy::FillFirst);
        pool.add_credential(make_cred_with_priority("low", 1));
        pool.add_credential(make_cred_with_priority("high", 100));
        pool.add_credential(make_cred_with_priority("mid", 50));

        // Should always pick highest priority first
        let r1 = pool.acquire().unwrap();
        assert_eq!(r1.id, "high");
        let r2 = pool.acquire().unwrap();
        assert_eq!(r2.id, "high");
    }

    #[test]
    fn test_rotation_on_rate_limit_error() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        // Simulate rate-limit: mark "a" as exhausted (as if 429 received)
        pool.mark_exhausted("a", 60_000);

        // Next acquire should rotate to "b"
        let r = pool.acquire().unwrap();
        assert_eq!(r.id, "b");
    }

    #[test]
    fn test_rotation_after_error_threshold() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        // Hit error threshold on "a" — should auto-exhaust and rotate to "b"
        for _ in 0..ERROR_THRESHOLD {
            pool.mark_error("a");
        }

        let r = pool.acquire().unwrap();
        assert_eq!(r.id, "b");
    }

    #[test]
    fn test_strategy_switch_round_robin_to_fill_first() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred_with_priority("low", 1));
        pool.add_credential(make_cred_with_priority("high", 10));

        // With RoundRobin, first is "low" (index 0)
        let r1 = pool.acquire().unwrap();
        assert_eq!(r1.id, "low");

        // Switch strategy
        pool.strategy = RotationStrategy::FillFirst;
        pool.release("low");

        // Now FillFirst should pick highest priority
        let r2 = pool.acquire().unwrap();
        assert_eq!(r2.id, "high");
    }

    #[test]
    fn test_is_available_checks_max_concurrent() {
        let mut cred = make_cred("a");
        cred.max_concurrent = 2;
        assert!(CredentialPool::is_available(&cred));

        cred.active_requests = 1;
        assert!(CredentialPool::is_available(&cred));

        cred.active_requests = 2;
        assert!(!CredentialPool::is_available(&cred));
    }

    #[test]
    fn test_is_available_permanently_exhausted() {
        let mut cred = make_cred("a");
        cred.is_exhausted = true;
        // No exhausted_until_ms → permanently exhausted
        assert!(!CredentialPool::is_available(&cred));
    }

    #[test]
    fn test_is_available_cooldown_expired() {
        let mut cred = make_cred("a");
        cred.is_exhausted = true;
        // Set cooldown to 0 (already expired)
        cred.exhausted_until_ms = Some(0);
        assert!(CredentialPool::is_available(&cred));
    }

    #[test]
    fn test_stats_after_exhaustion() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));

        pool.mark_exhausted("a", 60_000);
        let stats = pool.stats();
        assert_eq!(stats.exhausted, 1);
        assert_eq!(stats.available, 1);

        let a_stats = stats.per_credential.iter().find(|s| s.id == "a").unwrap();
        assert!(a_stats.is_exhausted);
        let b_stats = stats.per_credential.iter().find(|s| s.id == "b").unwrap();
        assert!(!b_stats.is_exhausted);
    }

    #[test]
    fn test_concurrent_limit_single_credential() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        let mut a = make_cred("a");
        a.max_concurrent = 3;
        pool.add_credential(a);

        let _r1 = pool.acquire().unwrap();
        let _r2 = pool.acquire().unwrap();
        let _r3 = pool.acquire().unwrap();

        // At capacity
        assert!(pool.acquire().is_none());

        // Release one
        pool.release("a");
        let r4 = pool.acquire().unwrap();
        assert_eq!(r4.id, "a");
    }

    #[test]
    fn test_fill_first_same_priority_prefers_lower_index() {
        let mut pool = CredentialPool::new("p", RotationStrategy::FillFirst);
        pool.add_credential(make_cred_with_priority("a", 10));
        pool.add_credential(make_cred_with_priority("b", 10));

        // Same priority, should prefer index 0 ("a")
        let r = pool.acquire().unwrap();
        assert_eq!(r.id, "a");
    }

    #[test]
    fn test_remove_credential_adjusts_index() {
        let mut pool = CredentialPool::new("p", RotationStrategy::RoundRobin);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.add_credential(make_cred("c"));

        // Acquire to advance index to 2
        pool.acquire().unwrap(); // a, index -> 1
        pool.release("a");
        pool.acquire().unwrap(); // b, index -> 2

        // Remove "c" (index 2), current_index=2 should be adjusted
        pool.remove_credential("c");
        assert_eq!(pool.total_count(), 2);
        // Pool should still function
        pool.release("b");
        let r = pool.acquire().unwrap();
        assert!(r.id == "a" || r.id == "b");
    }

    #[test]
    fn test_strategy_display_and_parse_roundtrip() {
        for strategy in &[
            RotationStrategy::RoundRobin,
            RotationStrategy::FillFirst,
            RotationStrategy::Random,
            RotationStrategy::LeastUsed,
        ] {
            let s = strategy.to_string();
            let parsed: RotationStrategy = s.parse().unwrap();
            assert_eq!(*strategy, parsed);
        }
    }

    #[test]
    fn test_rotation_strategy_alternate_parsing() {
        assert_eq!(
            "round-robin".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::RoundRobin
        );
        assert_eq!(
            "fill-first".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::FillFirst
        );
        assert_eq!(
            "least-used".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::LeastUsed
        );
        assert_eq!(
            "ROUNDROBIN".parse::<RotationStrategy>().unwrap(),
            RotationStrategy::RoundRobin
        );
    }

    #[test]
    fn test_config_default_strategy_is_round_robin() {
        let cfg = CredentialPoolConfig {
            strategy: None,
            credentials: vec![CredentialConfig {
                api_key: "sk-test".to_string(),
                ..Default::default()
            }],
        };
        let pool = cfg.to_pool("test");
        assert_eq!(pool.strategy, RotationStrategy::RoundRobin);
    }

    #[test]
    fn test_least_used_across_multiple_acquires() {
        let mut pool = CredentialPool::new("p", RotationStrategy::LeastUsed);
        pool.add_credential(make_cred("a"));
        pool.add_credential(make_cred("b"));
        pool.add_credential(make_cred("c"));

        // Acquire 6 times, releasing between to see LeastUsed balance
        let r1 = pool.acquire().unwrap(); // a (0,0,0) -> a gets 1
        pool.release(&r1.id);
        let r2 = pool.acquire().unwrap(); // b (1,0,0) -> b gets 1
        pool.release(&r2.id);
        let r3 = pool.acquire().unwrap(); // c (1,1,0) -> c gets 1
        pool.release(&r3.id);

        // Now all at 1 request each, next should be "a" (tie-break by index)
        let r4 = pool.acquire().unwrap();
        assert_eq!(r4.id, "a");
    }
}
