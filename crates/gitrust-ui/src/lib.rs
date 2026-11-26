//! Native, headlessly testable egui presentation layer for gitRUST.

mod components;
mod demo;
mod diff;
mod gallery;
mod i18n;
mod shell;
mod theme;

pub use i18n::Language;
pub use shell::{Desktop, DiagnosticView, FrameEvents, UiOptions};
