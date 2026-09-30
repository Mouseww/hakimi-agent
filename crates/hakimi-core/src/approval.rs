//! Human-in-the-loop (HITL) approval for dangerous tool calls.
//!
//! Guardrails (`crate::guardrails`) are *automatic* — they detect loops and
//! halt them without asking anyone. This module is the opposite: it stops
//! **before** a mutating tool runs and waits for a human decision.
//!
//! Design rules (see `docs/ARCHITECTURE.md` §4):
//!
//! 1. **Fail closed.** A timeout, a dropped resolver, or an unsupported
//!    platform all resolve to *denied*. Silence is never consent.
//! 2. **Explicit danger list.** Only tools that mutate the machine or execute
//!    code are gated by default. Read-only tools never pay the latency cost.
//! 3. **Observable.** Every request is surfaced as an `approval_request` event
//!    so the CLI, gateway, or API surface can render it and reply.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::oneshot;
use tracing::{debug, info, warn};

/// The decision a caller makes about one pending tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOutcome {
    /// Run the tool.
    Approved,
    /// Do not run the tool; the model receives a denial result.
    Denied,
}

/// What the gate tells the agent loop to do with a tool call.
///
/// Not `PartialEq`: [`ApprovalDecision::Pending`] owns a `oneshot::Receiver`,
/// which is neither `Clone` nor `Eq`. Match on the variant instead.
#[derive(Debug)]
pub enum ApprovalDecision {
    /// Not gated — dispatch immediately.
    NotRequired,
    /// Gated, but the operator pre-approved this tool.
    PreApproved,
    /// Gated, but nobody can answer (no surface attached). Denied.
    Unavailable(String),
    /// Gated and waiting on a human. The loop awaits `receiver`.
    Pending {
        /// Correlation id echoed back by `resolve`.
        request_id: String,
        /// The prompt to show the human.
        prompt: String,
        /// Receives the human's decision.
        receiver: oneshot::Receiver<ApprovalOutcome>,
    },
}

/// Policy controlling which tools need approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalPolicy {
    /// Master switch. When false the gate always returns `NotRequired`.
    pub enabled: bool,
    /// Tools that require approval when `enabled` is true.
    pub dangerous_tools: Vec<String>,
    /// Tools that skip approval even when listed as dangerous.
    pub auto_approve: Vec<String>,
    /// How long to wait for a human before failing closed.
    pub timeout: Duration,
    /// When true, a request with no attached surface is denied rather than
    /// silently allowed. Keep this true outside development.
    pub fail_closed: bool,
}

impl Default for ToolApprovalPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            dangerous_tools: Self::default_dangerous_tools(),
            auto_approve: Vec::new(),
            timeout: Duration::from_secs(300),
            fail_closed: true,
        }
    }
}

impl ToolApprovalPolicy {
    /// The tools gated by default: anything that mutates the host or runs code.
    ///
    /// Adding a new mutating tool means adding it here, with a test.
    pub fn default_dangerous_tools() -> Vec<String> {
        vec![
            "terminal".to_string(),
            "write_file".to_string(),
            "patch".to_string(),
            "code_exec".to_string(),
            "computer_use".to_string(),
        ]
    }

    /// Whether `tool` falls under this policy's danger list.
    ///
    /// Auto-approved tools still report `true` here — they are *gated*, they
    /// just skip the human wait. [`ApprovalGate::request`] downgrades them to
    /// [`ApprovalDecision::PreApproved`]; use [`Self::is_auto_approved`] when
    /// you need to tell "not gated" from "pre-approved" without a request.
    pub fn requires_approval(&self, tool: &str) -> bool {
        self.enabled && self.dangerous_tools.iter().any(|t| t == tool)
    }

    /// Whether the operator pre-approved `tool`, skipping the human wait.
    pub fn is_auto_approved(&self, tool: &str) -> bool {
        self.auto_approve.iter().any(|t| t == tool)
    }

    /// Build a permissive policy for development and tests.
    pub fn permissive() -> Self {
        Self {
            enabled: true,
            dangerous_tools: Self::default_dangerous_tools(),
            auto_approve: Self::default_dangerous_tools(),
            timeout: Duration::from_secs(5),
            fail_closed: false,
        }
    }
}

/// Tracks pending approval requests for one agent.
///
/// Shared behind an `Arc` between the agent loop (which creates requests) and
/// whatever surface answers them (CLI prompt, Telegram inline keyboard, HTTP
/// endpoint).
#[derive(Debug)]
pub struct ApprovalGate {
    policy: ToolApprovalPolicy,
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalOutcome>>>,
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new(ToolApprovalPolicy::default())
    }
}

impl ApprovalGate {
    /// Create a gate with the given policy.
    pub fn new(policy: ToolApprovalPolicy) -> Self {
        Self {
            policy,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// The active policy.
    pub fn policy(&self) -> &ToolApprovalPolicy {
        &self.policy
    }

    /// Replace the policy at runtime (config reload).
    pub fn set_policy(&mut self, policy: ToolApprovalPolicy) {
        self.policy = policy;
    }

    /// Number of requests currently waiting for a human.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().map(|p| p.len()).unwrap_or(0)
    }

    /// Register a request for `tool`.
    ///
    /// Returns [`ApprovalDecision::Pending`] with a receiver the caller awaits.
    /// Returns `Unavailable` when the policy fails closed and no surface can
    /// answer — the caller must deny.
    pub fn request(
        &self,
        request_id: impl Into<String>,
        tool: &str,
        args_summary: &str,
        has_surface: bool,
    ) -> ApprovalDecision {
        if !self.policy.requires_approval(tool) {
            return ApprovalDecision::NotRequired;
        }
        if self.policy.is_auto_approved(tool) {
            return ApprovalDecision::PreApproved;
        }
        if !has_surface {
            let reason = if self.policy.fail_closed {
                format!(
                    "tool '{tool}' requires human approval but no approval surface is attached \
                     (fail-closed policy)"
                )
            } else {
                format!("tool '{tool}' auto-approved: no approval surface attached")
            };
            if self.policy.fail_closed {
                warn!(tool, "approval unavailable; denying");
                return ApprovalDecision::Unavailable(reason);
            }
            return ApprovalDecision::PreApproved;
        }

        let request_id = request_id.into();
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(request_id.clone(), tx);
        }
        let prompt = format!(
            "Approve tool `{tool}`{args_summary}? Reply `/approve {request_id}` or \
             `/deny {request_id}`."
        );
        info!(tool, request_id = %request_id, "awaiting human approval");
        ApprovalDecision::Pending {
            request_id,
            prompt,
            receiver: rx,
        }
    }

    /// Resolve a pending request. Returns false when the id is unknown, which
    /// happens on timeout or a double reply.
    pub fn resolve(&self, request_id: &str, outcome: ApprovalOutcome) -> bool {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(request_id));
        match sender {
            Some(tx) => {
                debug!(request_id, ?outcome, "approval resolved");
                tx.send(outcome).is_ok()
            }
            None => {
                warn!(request_id, "approval resolved for unknown request id");
                false
            }
        }
    }

    /// Await a decision, failing closed on timeout or a dropped sender.
    pub async fn await_decision(
        receiver: oneshot::Receiver<ApprovalOutcome>,
        timeout: Duration,
    ) -> ApprovalOutcome {
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => {
                warn!("approval channel closed before a decision arrived; denying");
                ApprovalOutcome::Denied
            }
            Err(_) => {
                warn!(
                    timeout_secs = timeout.as_secs(),
                    "approval timed out; denying (fail closed)"
                );
                ApprovalOutcome::Denied
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_policy() -> ToolApprovalPolicy {
        ToolApprovalPolicy {
            enabled: true,
            ..ToolApprovalPolicy::default()
        }
    }

    #[test]
    fn disabled_policy_never_gates() {
        let gate = ApprovalGate::default();
        assert!(matches!(
            gate.request("r1", "terminal", "", true),
            ApprovalDecision::NotRequired
        ));
    }

    #[test]
    fn read_only_tools_are_not_gated() {
        let gate = ApprovalGate::new(enabled_policy());
        assert!(matches!(
            gate.request("r1", "read_file", "", true),
            ApprovalDecision::NotRequired
        ));
    }

    #[test]
    fn dangerous_tool_without_surface_fails_closed() {
        let gate = ApprovalGate::new(enabled_policy());
        match gate.request("r1", "terminal", "", false) {
            ApprovalDecision::Unavailable(reason) => assert!(reason.contains("terminal")),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn auto_approve_short_circuits() {
        let mut policy = enabled_policy();
        policy.auto_approve = vec!["terminal".to_string()];
        let gate = ApprovalGate::new(policy);
        assert!(matches!(
            gate.request("r1", "terminal", "", false),
            ApprovalDecision::PreApproved
        ));
    }

    #[tokio::test]
    async fn approval_round_trip() {
        let gate = std::sync::Arc::new(ApprovalGate::new(enabled_policy()));
        let decision = gate.request("req-1", "terminal", " (command: ls)", true);
        let (request_id, receiver) = match decision {
            ApprovalDecision::Pending {
                request_id,
                receiver,
                ..
            } => (request_id, receiver),
            other => panic!("expected Pending, got {other:?}"),
        };
        assert_eq!(request_id, "req-1");
        assert_eq!(gate.pending_count(), 1);
        assert!(gate.resolve("req-1", ApprovalOutcome::Approved));
        assert_eq!(gate.pending_count(), 0);
        let outcome = ApprovalGate::await_decision(receiver, Duration::from_secs(1)).await;
        assert_eq!(outcome, ApprovalOutcome::Approved);
    }

    #[tokio::test]
    async fn timeout_fails_closed() {
        let gate = ApprovalGate::new(enabled_policy());
        let decision = gate.request("req-2", "terminal", "", true);
        let receiver = match decision {
            ApprovalDecision::Pending { receiver, .. } => receiver,
            other => panic!("expected Pending, got {other:?}"),
        };
        let outcome = ApprovalGate::await_decision(receiver, Duration::from_millis(10)).await;
        assert_eq!(outcome, ApprovalOutcome::Denied);
    }

    #[test]
    fn resolving_unknown_id_is_reported() {
        let gate = ApprovalGate::new(enabled_policy());
        assert!(!gate.resolve("nope", ApprovalOutcome::Approved));
    }

    #[test]
    fn default_dangerous_tools_cover_mutating_tools() {
        let tools = ToolApprovalPolicy::default_dangerous_tools();
        for expected in ["terminal", "write_file", "patch", "code_exec"] {
            assert!(
                tools.iter().any(|t| t == expected),
                "{expected} must be gated by default"
            );
        }
    }
}
