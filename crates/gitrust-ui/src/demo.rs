//! Read-only fixtures used exclusively by the explicitly enabled diagnostics mode.

use crate::components::Row;
use crate::diff::{Diff, Line, TextDiff};
use crate::theme::FileStatus;

pub(crate) const REPOSITORY: &str = "signal-lab";
pub(crate) const BRANCHES: [&str; 3] = ["workbench", "main", "empty-history"];

#[derive(Clone, Copy, Debug)]
pub(crate) struct File {
    pub(crate) name: &'static str,
    pub(crate) path: &'static str,
    pub(crate) status: FileStatus,
}

impl File {
    pub(crate) const fn row(self) -> Row<'static> {
        Row {
            name: self.name,
            detail: self.path,
            status: Some(self.status),
        }
    }
}

pub(crate) const FILES: [File; 7] = [
    File {
        name: "pipeline.rs",
        path: "src/processing",
        status: FileStatus::Modified,
    },
    File {
        name: "fixture.rs",
        path: "tests/support",
        status: FileStatus::Added,
    },
    File {
        name: "legacy.rs",
        path: "src/processing",
        status: FileStatus::Deleted,
    },
    File {
        name: "sample.bin",
        path: "fixtures/audio",
        status: FileStatus::Untracked,
    },
    File {
        name: "routing.toml",
        path: "config",
        status: FileStatus::Conflicted,
    },
    File {
        name: "protocol.md",
        path: "docs/transport",
        status: FileStatus::Renamed,
    },
    File {
        name: "limits.toml",
        path: "config/validation",
        status: FileStatus::Modified,
    },
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct Commit {
    pub(crate) subject: &'static str,
    pub(crate) message: &'static str,
    pub(crate) short: &'static str,
    pub(crate) hash: &'static str,
    pub(crate) author: &'static str,
    pub(crate) date: &'static str,
}

pub(crate) const COMMITS: [Commit; 5] = [
    Commit {
        subject: "Keep sample ingestion within a fixed budget",
        message: "Set an upper bound on the sample buffer and reject frames above that limit before decoding.",
        short: "6c718ad",
        hash: "6c718ad52423ad6d988fca2805bca4c75b4fd44d",
        author: "Avery Kim",
        date: "2026-10-20 14:32 UTC",
    },
    Commit {
        subject: "Document the recording format",
        message: "Describe the header, expected byte order and checksum in the transport guide.",
        short: "c94a9fe",
        hash: "c94a9fe8df12a513a451795d00eb5812610d240d",
        author: "Ira Santos",
        date: "2026-10-19 10:06 UTC",
    },
    Commit {
        subject: "Share buffer limits with the validator",
        message: "Read the same limits for validation and processing so both paths report consistent errors.",
        short: "891b2e4",
        hash: "891b2e4c7c52925efdf89980ef88a2c3515539a1",
        author: "Avery Kim",
        date: "2026-10-18 16:17 UTC",
    },
    Commit {
        subject: "Add a real-time sample fixture",
        message: "Add a small recording to exercise the decoder with variable frame lengths.",
        short: "a81b522",
        hash: "a81b522362eb75445f18c31a60f0820493bf8b63",
        author: "Ira Santos",
        date: "2026-10-17 11:20 UTC",
    },
    Commit {
        subject: "Initialize the processing workspace",
        message: "Set up the package and the first tests for sample ingestion.",
        short: "3b21986",
        hash: "3b219861753fe804c83dd949c719eadcc37e87da",
        author: "Avery Kim",
        date: "2026-10-16 15:09 UTC",
    },
];

#[derive(Debug)]
pub(crate) struct Fixtures {
    pipeline: TextDiff,
    added: TextDiff,
    removed: TextDiff,
    long: TextDiff,
}

impl Default for Fixtures {
    fn default() -> Self {
        use Line::{Change, Context, Hunk};
        Self {
            pipeline: TextDiff::new(vec![
                Hunk("@@ -24,7 +24,11 @@ Sample ingestion"),
                Context(
                    24,
                    24,
                    "pub(crate) fn ingest(frame: &Frame) -> Result<Sample> {",
                ),
                Context(25, 25, "    let payload = frame.payload();"),
                Change {
                    old: Some((26, "    let limit = 4_096;")),
                    new: Some((26, "    let limit = 16_384;")),
                },
                Context(27, 27, ""),
                Change {
                    old: None,
                    new: Some((28, "    if payload.len() > limit {")),
                },
                Change {
                    old: None,
                    new: Some((29, "        return Err(Error::FrameTooLarge);")),
                },
                Change {
                    old: None,
                    new: Some((30, "    }")),
                },
                Change {
                    old: None,
                    new: Some((31, "")),
                },
                Change {
                    old: Some((28, "    decode(payload)")),
                    new: Some((32, "    decode_checked(payload)")),
                },
                Context(29, 33, "}"),
                Context(30, 34, ""),
                Hunk("@@ -44,3 +48,3 @@ Decoder"),
                Change {
                    old: Some((44, "fn decode(data: &[u8]) -> Result<Sample> {")),
                    new: Some((48, "fn decode_checked(data: &[u8]) -> Result<Sample> {")),
                },
                Context(45, 49, "    Sample::try_from(data)"),
                Context(46, 50, "}"),
            ]),
            added: TextDiff::new(vec![
                Hunk("@@ -0,0 +1,3 @@"),
                Change {
                    old: None,
                    new: Some((1, "pub(crate) fn sample() -> &'static [u8] {")),
                },
                Change {
                    old: None,
                    new: Some((2, "    &[0x10, 0x20, 0x30]")),
                },
                Change {
                    old: None,
                    new: Some((3, "}")),
                },
            ]),
            removed: TextDiff::new(vec![
                Hunk("@@ -1,3 +0,0 @@"),
                Change {
                    old: Some((1, "fn accept_unchecked(data: &[u8]) {")),
                    new: None,
                },
                Change {
                    old: Some((2, "    legacy_decoder(data);")),
                    new: None,
                },
                Change {
                    old: Some((3, "}")),
                    new: None,
                },
            ]),
            long: TextDiff::new(vec![
                Hunk("@@ -2 +2 @@ validation"),
                Change {
                    old: Some((
                        2,
                        "fallback\t= [\"gravação-alpha\", \"recording-beta\", \"recording-gamma\", \"recording-delta\", \"recording-epsilon\", \"recording-zeta\", \"recording-eta\", \"recording-theta\"]",
                    )),
                    new: Some((
                        2,
                        "fallback\t= [\"gravação-alpha\", \"recording-beta\", \"recording-gamma\", \"recording-delta\", \"recording-epsilon\", \"recording-zeta\", \"recording-eta\", \"recording-production\"]",
                    )),
                },
            ]),
        }
    }
}

impl Fixtures {
    pub(crate) fn file(&self, index: usize) -> Diff<'_> {
        match index {
            0 => Diff::Text(&self.pipeline),
            1 => Diff::Text(&self.added),
            2 => Diff::Text(&self.removed),
            3 => Diff::Binary,
            6 => Diff::Text(&self.long),
            _ => Diff::Empty,
        }
    }
}
