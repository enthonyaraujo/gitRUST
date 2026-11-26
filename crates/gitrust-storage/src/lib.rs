//! Local preferences, workspace history and credential-handling policy for gitRUST.

mod credentials;
mod persistence;
pub use credentials::{CredentialError, CredentialVault};
pub use persistence::{GithubBindings, StorageError, Store};

use gitrust_core::CrateStageStatus;

/// Current implementation readiness of `gitrust-storage`.
pub const STAGE_STATUS: CrateStageStatus = CrateStageStatus::Active { stage: 3 };

/// Policy governing how sensitive authentication material may be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStoragePolicy {
    /// Store secrets exclusively through native OS credential managers
    /// (Secret Service / libsecret on Linux, Credential Manager on Windows, Keychain on macOS)
    /// or delegate to the user's configured Git credential helper.
    OsKeychainOrGitHelperOnly,
}

/// Baseline privacy and storage policy enforced across all stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageSecurityBaseline {
    /// Plaintext credential persistence is unconditionally prohibited.
    pub allow_plaintext_credentials: bool,
    /// Telemetry collection is disabled by default.
    pub telemetry_enabled_by_default: bool,
    /// Approved secret storage mechanism.
    pub credential_policy: CredentialStoragePolicy,
}

impl Default for StorageSecurityBaseline {
    fn default() -> Self {
        Self::strict()
    }
}

impl StorageSecurityBaseline {
    /// Returns the mandatory security baseline for `gitRUST`.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            allow_plaintext_credentials: false,
            telemetry_enabled_by_default: false,
            credential_policy: CredentialStoragePolicy::OsKeychainOrGitHelperOnly,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_prohibits_plaintext_secrets_and_telemetry() {
        let baseline = StorageSecurityBaseline::default();
        assert!(!baseline.allow_plaintext_credentials);
        assert!(!baseline.telemetry_enabled_by_default);
        assert_eq!(
            baseline.credential_policy,
            CredentialStoragePolicy::OsKeychainOrGitHelperOnly
        );
    }
}
