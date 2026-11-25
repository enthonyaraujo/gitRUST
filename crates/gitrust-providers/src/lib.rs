//! Remote hosting provider boundary (`GitHub`, `GitLab`, and self-hosted instances).
//!
//! # Stage 00 Status
//! Provider integrations are **not implemented** in Stage 00 and are scheduled
//! for **Stage 08 (Provider Integrations)**. No HTTP client or network stack is
//! compiled into the workspace during Stage 00.

use gitrust_core::CrateStageStatus;

/// Current implementation readiness of `gitrust-providers`.
pub const STAGE_STATUS: CrateStageStatus = CrateStageStatus::Planned { target_stage: 8 };

/// Transport security policy for all future remote provider connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVerificationPolicy {
    /// Strict certificate chain and hostname verification is mandatory; disabling
    /// TLS verification is prohibited by the security architecture.
    StrictCertificateVerification,
}

/// Returns the mandatory transport security policy for provider integrations.
#[must_use]
pub const fn required_tls_policy() -> TlsVerificationPolicy {
    TlsVerificationPolicy::StrictCertificateVerification
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_strict_tls_verification_policy() {
        assert_eq!(
            required_tls_policy(),
            TlsVerificationPolicy::StrictCertificateVerification
        );
    }
}
