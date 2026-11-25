//! Workspace-level integration tests verifying cross-crate contracts and CLI behavior.

use std::process::Command;

use gitrust_core::{APP_LICENSE, APP_NAME, AppMetadata, CrateStageStatus, PACKAGE_NAME};
use gitrust_git::{GitCommandSpec, OperationSafetyClass};
use gitrust_providers::TlsVerificationPolicy;
use gitrust_storage::StorageSecurityBaseline;

#[test]
fn workspace_crates_preserve_stage_00_honesty_and_security_baselines() {
    let meta = AppMetadata::new(env!("CARGO_PKG_VERSION"));
    assert_eq!(meta.name, APP_NAME);
    assert_eq!(meta.package_name, PACKAGE_NAME);
    assert_eq!(meta.license, APP_LICENSE);

    assert_eq!(
        gitrust_git::STAGE_STATUS,
        CrateStageStatus::Planned { target_stage: 2 }
    );
    assert_eq!(
        gitrust_storage::STAGE_STATUS,
        CrateStageStatus::Planned { target_stage: 3 }
    );
    assert_eq!(
        gitrust_providers::STAGE_STATUS,
        CrateStageStatus::Planned { target_stage: 8 }
    );

    let storage_policy = StorageSecurityBaseline::strict();
    assert!(!storage_policy.allow_plaintext_credentials);
    assert!(!storage_policy.telemetry_enabled_by_default);

    assert_eq!(
        gitrust_providers::required_tls_policy(),
        TlsVerificationPolicy::StrictCertificateVerification
    );

    let spec = GitCommandSpec::new(
        std::env::temp_dir(),
        "status",
        ["--porcelain=v2"],
        OperationSafetyClass::ReadOnly,
    );
    assert!(spec.is_ok());
}

#[test]
fn binary_reports_cargo_version_and_help_without_error() {
    let bin = env!("CARGO_BIN_EXE_gitrust");

    let version_out = Command::new(bin).arg("--version").output();
    assert!(version_out.is_ok());
    if let Ok(output) = version_out {
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            stdout.trim(),
            format!("gitRUST {}", env!("CARGO_PKG_VERSION"))
        );
    }

    let help_out = Command::new(bin).arg("--help").output();
    assert!(help_out.is_ok());
    if let Ok(output) = help_out {
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("USAGE:"));
        assert!(stdout.contains("--smoke-test"));
    }
}
