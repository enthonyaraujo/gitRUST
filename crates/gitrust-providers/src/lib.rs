//! Native hosting provider clients.

pub mod github;

use gitrust_core::CrateStageStatus;

/// Current implementation readiness of `gitrust-providers`.
pub const STAGE_STATUS: CrateStageStatus = CrateStageStatus::Active { stage: 7 };

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
