use std::time::Duration;

use egui::{Context, Event, Pos2, RawInput, Rect, Theme, ViewportId, vec2};

use super::*;
use gitrust_core::workspaces::RepositoryAction;

fn state() -> AppState {
    AppState::new(
        gitrust_core::AppMetadata::new("test"),
        gitrust_core::HostDiagnostics::from_protocol(
            gitrust_core::WindowSessionProtocol::HeadlessOrUnknown,
        ),
        gitrust_core::GraphicsDiagnostics::default(),
    )
}

fn render(ctx: &Context, shell: &mut Desktop, input: RawInput) -> egui::FullOutput {
    let mut state = state();
    let mut output = ctx.run_ui(input, |ui| {
        for event in shell.show(ui, &state).into_iter_events() {
            state.apply(event);
        }
    });
    output.textures_delta.clear();
    output
}

fn input(time: f64) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 400.0))),
        time: Some(time),
        ..Default::default()
    }
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn regular_workspace_has_no_fixtures_and_does_not_repaint_after_settling() {
    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    assert!(shell.demo.is_none());
    assert_eq!(shell.screen, Screen::Workspace);
    for time in [0.0, 0.1, 0.2, 0.3] {
        let output = render(&ctx, &mut shell, input(time));
        assert_ne!(output.shapes.len(), 0);
    }
    let output = render(&ctx, &mut shell, input(0.4));
    assert_eq!(
        output.viewport_output[&ViewportId::ROOT].repaint_delay,
        Duration::MAX
    );
}

#[test]
fn system_theme_follows_input_and_explicit_theme_overrides_it() {
    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    for theme in [Theme::Light, Theme::Dark] {
        let _ = render(
            &ctx,
            &mut shell,
            RawInput {
                system_theme: Some(theme),
                ..input(0.0)
            },
        );
        assert_eq!(ctx.theme(), theme);
    }
    ctx.set_theme(ThemePreference::Light);
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            system_theme: Some(Theme::Dark),
            ..input(0.1)
        },
    );
    assert_eq!(ctx.theme(), Theme::Light);
}

#[test]
fn keyboard_filters_selects_rows_and_opens_settings() {
    let ctx = Context::default();
    let mut shell = Desktop::new(
        &ctx,
        UiOptions {
            diagnostics: Some(DiagnosticView::Keyboard),
            ..UiOptions::default()
        },
    );
    let _ = render(&ctx, &mut shell, input(0.0));
    let _ = render(&ctx, &mut shell, input(0.1));
    let filter_id = Id::unique("workspace-filter");
    assert!(ctx.memory(|m| m.has_focus(filter_id)));
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::Tab, Modifiers::NONE)],
            ..input(0.2)
        },
    );
    let _ = render(&ctx, &mut shell, input(0.3));
    let list = shell.demo.as_ref().expect("diagnostic fixture");
    assert_eq!(list.files.index, Some(0));
    assert!(!ctx.memory(|m| m.has_focus(filter_id)));
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::ArrowDown, Modifiers::NONE)],
            ..input(0.4)
        },
    );
    assert_eq!(
        shell.demo.as_ref().and_then(|demo| demo.files.index),
        Some(1)
    );
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::Comma, Modifiers::COMMAND)],
            ..input(0.5)
        },
    );
    assert_eq!(shell.dialog, Dialog::Settings);
}

#[test]
fn diagnostic_composer_focus_targets_the_requested_text_editor() {
    for (view, field) in [
        (DiagnosticView::ComposeSummary, "summary"),
        (DiagnosticView::ComposeDescription, "description"),
    ] {
        let ctx = Context::default();
        let mut shell = Desktop::new(
            &ctx,
            UiOptions {
                diagnostics: Some(view),
                ..UiOptions::default()
            },
        );
        let _ = render(&ctx, &mut shell, input(0.0));
        let output = render(
            &ctx,
            &mut shell,
            RawInput {
                events: vec![Event::Text("edição 日本語".into())],
                ..input(0.1)
            },
        );
        assert!(ctx.memory(|memory| memory.has_focus(Id::unique(("modal-draft", field)))));
        assert!(output.platform_output.ime.is_some());
        let draft = shell.demo.expect("draft diagnostics");
        match view {
            DiagnosticView::ComposeSummary => {
                assert_eq!(draft.summary, "edição 日本語");
                assert_eq!(draft.description, "");
            }
            DiagnosticView::ComposeDescription => {
                assert_eq!(draft.description, "edição 日本語");
                assert_eq!(draft.summary, "");
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn tab_shortcuts_switch_views_without_editing_the_focused_filter() {
    let ctx = Context::default();
    let mut shell = Desktop::new(
        &ctx,
        UiOptions {
            diagnostics: Some(DiagnosticView::Keyboard),
            ..UiOptions::default()
        },
    );
    let _ = render(&ctx, &mut shell, input(0.0));
    let _ = render(&ctx, &mut shell, input(0.1));
    assert!(ctx.memory(|m| m.has_focus(Id::unique("workspace-filter"))));
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![Event::Text("processing".into())],
            ..input(0.15)
        },
    );
    let changes_matches = shell
        .demo
        .as_ref()
        .expect("diagnostic fixture")
        .matches
        .clone();
    assert_eq!(changes_matches, [0, 2]);

    for (time, digit, key_code, tab) in [
        (0.2, "2", Key::Num2, Tab::History),
        (0.3, "1", Key::Num1, Tab::Changes),
    ] {
        let _ = render(
            &ctx,
            &mut shell,
            RawInput {
                events: vec![key(key_code, Modifiers::ALT), Event::Text(digit.into())],
                ..input(time)
            },
        );
        assert_eq!(shell.tab, tab);
        let demo = shell.demo.as_ref().expect("diagnostic fixture");
        assert_eq!(demo.query, "processing");
        if tab == Tab::History {
            assert_eq!(demo.matches.len(), 1);
        } else {
            assert_eq!(demo.matches, changes_matches);
        }
    }

    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::Num2, Modifiers::NONE), Event::Text("2".into())],
            ..input(0.4)
        },
    );
    assert_eq!(shell.tab, Tab::Changes);
    assert_eq!(
        shell.demo.as_ref().expect("diagnostic fixture").query,
        "processing2"
    );
}

#[test]
fn narrow_portuguese_settings_keep_the_close_button_in_view() {
    let ctx = Context::default();
    let mut shell = Desktop::new(
        &ctx,
        UiOptions {
            language: Language::Portuguese,
            zoom: 2.0,
            diagnostics: Some(DiagnosticView::Settings),
            ..UiOptions::default()
        },
    );
    ctx.enable_accesskit();
    let mut observed = false;
    for time in [0.0, 0.1, 0.2, 0.3] {
        let output = render(&ctx, &mut shell, input(time));
        let limit = ctx.content_rect();
        if let Some(update) = output.platform_output.accesskit_update {
            observed |= update.nodes.iter().any(|(_, node)| {
                node.label() == Some("Fechar")
                    && node.bounds().is_some_and(|rect| {
                        rect.x0 >= f64::from(limit.left())
                            && rect.x1 <= f64::from(limit.right())
                            && rect.y0 >= f64::from(limit.top())
                            && rect.y1 <= f64::from(limit.bottom())
                    })
            });
        }
    }
    assert!(
        observed,
        "close button must be reachable in the Portuguese dialog"
    );
}

#[test]
#[ignore = "instrumented CPU benchmark; run in release with --ignored --nocapture"]
fn profile_virtualized_views() {
    use crate::diff::{Diff, DiffView};
    use std::time::Instant;

    let ctx = Context::default();
    theme::install(&ctx);
    let palette = Palette::for_theme(Theme::Dark);
    let language = Language::English;
    for count in [100, 1_000, 10_000, 100_000] {
        let mut list = ListSelection::default();
        let mut diff = DiffView::default();
        let mut samples = Vec::with_capacity(100);
        for frame in 0..110 {
            let now = Instant::now();
            let mut output = ctx.run_ui(input(f64::from(frame) / 60.0), |ui| {
                if count == 100_000 {
                    diff.show(
                        ui,
                        Id::unique("benchmark-diff"),
                        Diff::Stress(count),
                        &mut DiffMode::Unified,
                        language,
                        palette,
                    );
                } else {
                    list.show(
                        ui,
                        Id::unique("benchmark-list"),
                        400.0,
                        count,
                        palette,
                        language,
                        |index| index,
                        |_| Row {
                            name: "signal.rs",
                            detail: "src/processing",
                            status: Some(FileStatus::Modified),
                        },
                    );
                }
            });
            let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
            output.textures_delta.clear();
            if frame >= 10 {
                samples.push(now.elapsed());
            }
        }
        samples.sort_unstable();
        eprintln!(
            "{count:>6} entries: median {:.3} ms; p95 {:.3} ms",
            samples[50].as_secs_f64() * 1_000.0,
            samples[94].as_secs_f64() * 1_000.0
        );
    }
}

#[test]
fn view_menu_opens_gallery_when_diagnostics_are_enabled() {
    let ctx = Context::default();
    ctx.enable_accesskit();
    let mut shell = Desktop::new(
        &ctx,
        UiOptions {
            diagnostics: Some(DiagnosticView::Unified),
            ..UiOptions::default()
        },
    );
    let mut time = 0.0;
    let mut frame = |shell: &mut Desktop, events| {
        time += 0.1;
        render(
            &ctx,
            shell,
            RawInput {
                events,
                ..input(time)
            },
        )
    };
    let position = |output: egui::FullOutput, name| {
        let update = output
            .platform_output
            .accesskit_update
            .expect("AccessKit tree update");
        update
            .nodes
            .into_iter()
            .find_map(|(_, node)| {
                let bounds = node.bounds()?;
                if node.label() != Some(name) {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation)]
                Some(egui::pos2(
                    f64::midpoint(bounds.x0, bounds.x1) as f32,
                    f64::midpoint(bounds.y0, bounds.y1) as f32,
                ))
            })
            .expect("label must be visible")
    };
    let menu_pos = position(frame(&mut shell, Vec::new()), "View");
    let mut click = |shell: &mut Desktop, pos| {
        let _ = frame(shell, vec![Event::PointerMoved(pos)]);
        let button = |pressed| Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        };
        let _ = frame(shell, vec![button(true)]);
        let _ = frame(shell, vec![button(false)]);
        frame(shell, Vec::new())
    };
    let gallery_pos = position(click(&mut shell, menu_pos), "Component gallery");
    let _ = click(&mut shell, gallery_pos);
    assert_eq!(shell.screen, Screen::Gallery);
}

#[test]
fn filters_keep_a_selected_file_key_only_while_it_is_present() {
    let mut demo = Demo::default();
    demo.filter(Tab::Changes);
    demo.files.index = Some(1);
    demo.query = "fixture.rs".into();
    demo.filter(Tab::Changes);
    assert_eq!(demo.matches, [1]);
    assert_eq!(demo.files.index, Some(1));
    demo.query = "legacy".into();
    demo.filter(Tab::Changes);
    assert_eq!(demo.matches, [2]);
    assert_eq!(demo.files.index, None);
}

#[test]
fn inline_demo_composer_exposes_unavailability() {
    let ctx = Context::default();
    ctx.enable_accesskit();
    let mut shell = Desktop::new(
        &ctx,
        UiOptions {
            diagnostics: Some(DiagnosticView::Unified),
            language: Language::Portuguese,
            ..UiOptions::default()
        },
    );
    let mut hint_bounds = None;
    let mut commit_disabled = false;
    let mut selected_file = false;
    for time in [0.0, 0.1, 0.2, 0.3] {
        let output = render(
            &ctx,
            &mut shell,
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 768.0))),
                time: Some(time),
                ..Default::default()
            },
        );
        if let Some(update) = output.platform_output.accesskit_update {
            for (_, node) in &update.nodes {
                if node.value() == Some("Rascunho local. Commit indisponível.") {
                    hint_bounds = node.bounds();
                }
                if node.label() == Some("Criar commit") {
                    commit_disabled |= node.is_disabled();
                }
                if node.label() == Some("pipeline.rs src/processing Modificado") {
                    selected_file |= node.is_selected() == Some(true) && node.toggled().is_none();
                }
            }
        }
    }
    assert!(
        hint_bounds.is_some_and(|rect| rect.y1 <= 768.0),
        "{hint_bounds:?}"
    );
    assert!(commit_disabled);
    assert!(selected_file);
}

#[test]
fn demo_views_stop_requesting_frames_at_rest() {
    for view in [
        DiagnosticView::Unified,
        DiagnosticView::Split,
        DiagnosticView::StressList,
        DiagnosticView::StressDiff,
    ] {
        let ctx = Context::default();
        let mut shell = Desktop::new(
            &ctx,
            UiOptions {
                diagnostics: Some(view),
                ..UiOptions::default()
            },
        );
        for step in 0..10 {
            let _ = render(&ctx, &mut shell, input(f64::from(step) * 0.25));
        }
        let output = render(&ctx, &mut shell, input(2.5));
        assert_eq!(
            output.viewport_output[&ViewportId::ROOT].repaint_delay,
            Duration::MAX,
            "{view:?}"
        );
    }
}

#[test]
fn desktop_accelerators_distinguish_shift_and_respect_zoom_limits() {
    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    shell.set_repository_view(gitrust_core::workspaces::RepositoryView {
        git_available: true,
        activity: gitrust_core::workspaces::Activity::Idle,
        ..Default::default()
    });
    let ctrl_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::O, ctrl_shift)],
            ..input(0.0)
        },
    );
    assert!(shell.managed.as_ref().is_some_and(ManagerUi::is_clone_form));
    let ctx = Context::default();

    let mut shell = Desktop::new(
        &ctx,
        UiOptions {
            diagnostics: Some(DiagnosticView::Keyboard),
            ..UiOptions::default()
        },
    );
    for (index, (key_code, zoom)) in [
        (Key::Equals, 1.25),
        (Key::Equals, 1.5),
        (Key::Equals, 2.0),
        (Key::Equals, 2.0),
        (Key::Num0, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let _ = render(
            &ctx,
            &mut shell,
            RawInput {
                events: vec![
                    key(key_code, Modifiers::COMMAND),
                    Event::Key {
                        key: key_code,
                        physical_key: Some(key_code),
                        pressed: false,
                        repeat: false,
                        modifiers: Modifiers::COMMAND,
                    },
                ],
                ..input(1.0 + f64::from(u32::try_from(index).expect("small index")))
            },
        );
        assert!((shell.options.zoom - zoom).abs() < f32::EPSILON);
    }
}

#[test]
#[allow(clippy::too_many_lines)] // Exercise the whole fetch lifecycle on one workspace.
fn toolbar_and_keyboard_fetch_origin_without_opening_configuration() {
    use gitrust_core::{
        github::{AccountId, AuthMethod, RepositoryKey},
        remotes::{Credential, Remote, RemoteAction, RemoteNotice, RemotesView},
        workspaces::{Activity, ChangeCounts, Checkout, RepositoryView, Workspace},
    };

    let ctx = Context::default();
    ctx.enable_accesskit();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    let mut view = RepositoryView {
        git_available: true,
        activity: Activity::Idle,
        active: Some(Workspace {
            path: "/fixture".into(),
            checkout: Checkout::Branch("main".into()),
            has_remote: true,
            changes: Some(ChangeCounts::default()),
        }),
        remotes: Some(RemotesView {
            version: 27,
            upstream: Some(("upstream".into(), "main".into())),
            ahead_behind: Some((3, 2)),
            entries: [
                Remote {
                    name: "upstream".into(),
                    fetch: Some("https://other.example/repository.git".into()),
                    push: None,
                },
                Remote {
                    name: "origin".into(),
                    fetch: Some("https://github.com/fixture/example.git".into()),
                    push: None,
                },
            ]
            .into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    shell.set_repository_view(view.clone());
    assert_fetch_label(
        &ctx,
        &mut shell,
        0.0,
        "Fetch origin, Last fetch: never",
        false,
    );
    let (time, target) = press_fetch_toolbar(&ctx, &mut shell);
    let Some(RepositoryAction::Remote(intent)) = shell.take_repository_action() else {
        panic!("click should submit a remote operation directly");
    };
    assert_eq!(intent.version, 27);
    assert!(matches!(
        intent.action,
        RemoteAction::Fetch(names, auth) if names == ["origin"] && matches!(auth.credential, Credential::Anonymous)
    ));

    assert_fetch_label(
        &ctx,
        &mut shell,
        time + 0.1,
        "Fetching origin, Hang on…",
        true,
    );
    click_fetch_target(&ctx, &mut shell, target, time + 0.11, 0.01);
    assert!(shell.take_repository_action().is_none());

    let last_fetch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("valid clock")
        .as_secs();
    let remotes = view.remotes.as_mut().expect("remote snapshot");
    remotes.last_fetch = [("origin".into(), last_fetch)].into();
    remotes.notice = Some(RemoteNotice::Fetched);
    shell.set_repository_view(view.clone());
    assert_fetch_label(
        &ctx,
        &mut shell,
        time + 0.2,
        "Fetching origin, Hang on…",
        true,
    );
    assert_fetch_label(
        &ctx,
        &mut shell,
        time + 0.6,
        "Fetch origin, Last fetch: just now",
        false,
    );

    view.github.workspace_account = Some(AccountId {
        method: AuthMethod::Browser,
        number: 1,
    });
    view.github.workspace_repo = RepositoryKey::new("fixture", "example");
    shell.set_repository_view(view.clone());
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::T, Modifiers::COMMAND.plus(Modifiers::SHIFT))],
            ..input(time + 0.7)
        },
    );
    let Some(RepositoryAction::Remote(intent)) = shell.take_repository_action() else {
        panic!("shortcut should submit a remote operation directly");
    };
    assert!(matches!(
        intent.action,
        RemoteAction::Fetch(names, auth) if names == ["origin"] && matches!(auth.credential, Credential::LinkedAccount)
    ));
    let remotes = view.remotes.as_mut().expect("remote snapshot");
    remotes.busy = true;
    remotes.notice = None;
    shell.set_repository_view(view.clone());
    assert_fetch_label(
        &ctx,
        &mut shell,
        time + 2.0,
        "Fetching origin, Hang on…",
        true,
    );
    let remotes = view.remotes.as_mut().expect("remote snapshot");
    remotes.busy = false;
    remotes.notice = Some(RemoteNotice::Offline);
    shell.set_repository_view(view);
    assert_fetch_label(
        &ctx,
        &mut shell,
        time + 2.1,
        "Fetch origin, Last fetch: just now",
        false,
    );
    assert!(matches!(
        shell.managed.as_ref().expect("manager").fetch,
        fetch::FetchState::Error
    ));
}

fn assert_fetch_label(ctx: &Context, shell: &mut Desktop, time: f64, label: &str, disabled: bool) {
    // AccessKit updates are incremental. Reset the tree to inspect the complete widget state.
    ctx.disable_accesskit();
    ctx.enable_accesskit();
    let output = render(ctx, shell, input(time));
    let update = output
        .platform_output
        .accesskit_update
        .expect("accessibility tree");
    let (_, button) = update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .expect(label);
    assert_eq!(button.is_disabled(), disabled);
}

fn press_fetch_toolbar(ctx: &Context, shell: &mut Desktop) -> (f64, Pos2) {
    let mut target = None;
    let mut time = 0.05;
    for _ in 0..4 {
        let update = render(ctx, shell, input(time))
            .platform_output
            .accesskit_update
            .expect("accessibility tree");
        target = update
            .nodes
            .iter()
            .find_map(|(_, node)| {
                node.label()
                    .is_some_and(|label| label.starts_with("Fetch origin,"))
                    .then(|| node.bounds())
                    .flatten()
            })
            .or(target);
        time += 0.1;
    }
    let bounds = target.expect("fetch origin toolbar control");
    #[allow(clippy::cast_possible_truncation)]
    let pos = Pos2::new(
        f64::midpoint(bounds.x0, bounds.x1) as f32,
        f64::midpoint(bounds.y0, bounds.y1) as f32,
    );
    (click_fetch_target(ctx, shell, pos, time, 0.1), pos)
}

fn click_fetch_target(
    ctx: &Context,
    shell: &mut Desktop,
    pos: Pos2,
    mut time: f64,
    step: f64,
) -> f64 {
    for pressed in [true, false] {
        let _ = render(
            ctx,
            shell,
            RawInput {
                events: vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: Modifiers::NONE,
                    },
                ],
                ..input(time)
            },
        );
        time += step;
    }
    time
}

#[test]
fn clean_workspace_shortcuts_dispatch_the_advertised_actions() {
    use gitrust_core::{
        github::{Action, RepositoryKey},
        workspaces::{Activity, Checkout, DesktopTool, RepositoryView, Workspace},
    };

    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    let path = std::path::PathBuf::from("/fixture");
    let mut view = RepositoryView {
        git_available: true,
        activity: Activity::Idle,
        active: Some(Workspace {
            path: path.clone(),
            checkout: Checkout::Branch("main".into()),
            has_remote: true,
            changes: None,
        }),
        ..Default::default()
    };
    view.github.workspace_repo = RepositoryKey::new("fixture", "example");
    shell.set_repository_view(view);
    let modifiers = Modifiers::CTRL
        .plus(Modifiers::COMMAND)
        .plus(Modifiers::SHIFT);
    let mut press = |key_code| {
        let _ = render(
            &ctx,
            &mut shell,
            RawInput {
                events: vec![key(key_code, modifiers)],
                ..input(0.0)
            },
        );
        shell.take_repository_action()
    };
    if cfg!(target_os = "linux") {
        assert!(matches!(
            press(Key::A),
            Some(RepositoryAction::OpenDesktopTool { tool: DesktopTool::Editor, path: actual })
                if actual == path
        ));
    }
    assert!(matches!(press(Key::F), Some(RepositoryAction::Reveal(actual)) if actual == path));
    assert!(matches!(
        press(Key::G),
        Some(RepositoryAction::Github(Action::Open(url))) if url == "https://github.com/fixture/example"
    ));
}

#[test]
fn empty_workspace_centers_the_visible_block_across_scales() {
    use gitrust_core::workspaces::{Activity, RecentRepositories, RepositoryView};

    for zoom in [1.0, 1.25, 1.5, 2.0] {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut shell = Desktop::new(
            &ctx,
            UiOptions {
                zoom,
                ..UiOptions::default()
            },
        );
        shell.enable_repository_management();
        shell.set_repository_view(RepositoryView {
            git_available: true,
            activity: Activity::Idle,
            recent: RecentRepositories {
                paths: vec![format!("/fixture/{}/Workspace", "long-directory/".repeat(8)).into()],
                ..Default::default()
            },
            ..Default::default()
        });
        let mut heading = None;
        let mut bottom = None;
        let mut toolbar = None;
        for time in [0.0, 0.1, 0.2] {
            let output = render(
                &ctx,
                &mut shell,
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 760.0))),
                    time: Some(time),
                    ..Default::default()
                },
            );
            if let Some(update) = output.platform_output.accesskit_update {
                for (_, node) in &update.nodes {
                    match node.label().or(node.value()) {
                        Some("No repository open") => heading = node.bounds(),
                        Some("Remove") => bottom = node.bounds(),
                        Some("Current repository, None open") => toolbar = node.bounds(),
                        _ => {}
                    }
                }
            }
        }
        let heading = heading.expect("empty workspace heading");
        let bottom = bottom.expect("recent workspace action");
        let toolbar = toolbar.expect("repository selector");
        let viewport = ctx.content_rect();
        assert!(
            (f64::midpoint(heading.x0, heading.x1) - f64::from(viewport.center().x)).abs() < 2.0,
            "heading is off-center at scale {zoom}: {heading:?}"
        );
        assert!(
            bottom.x1 <= f64::from(viewport.center().x + 176.0),
            "recent action extends beyond the centered block at scale {zoom}: {bottom:?}"
        );
        // The room above the heading includes the leading icon, and the trailing path
        // extends below the vertically centered Remove button.
        let top_gap = heading.y0 - toolbar.y1;
        let bottom_gap = f64::from(viewport.bottom()) - bottom.y1;
        assert!(
            (top_gap - bottom_gap - 20.0).abs() < 18.0,
            "block is off-center at scale {zoom}: top gap {top_gap}, bottom gap {bottom_gap}"
        );
    }
}

#[test]
fn empty_workspace_scrolls_to_recents_in_a_short_viewport() {
    use gitrust_core::workspaces::{Activity, RecentRepositories, RepositoryView};

    let ctx = Context::default();
    ctx.enable_accesskit();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    shell.set_repository_view(RepositoryView {
        git_available: true,
        activity: Activity::Idle,
        recent: RecentRepositories {
            paths: vec!["/fixture/Workspace".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    // This logical size represents a 680 x 420 window at 200% scale.
    let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(340.0, 210.0)));
    let mut last_remove = None;
    for (time, events) in [
        (0.0, vec![Event::PointerMoved(Pos2::new(200.0, 170.0))]),
        (
            0.1,
            vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -360.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            }],
        ),
        (0.2, Vec::new()),
        (0.3, Vec::new()),
        (0.4, Vec::new()),
    ] {
        let output = render(
            &ctx,
            &mut shell,
            RawInput {
                screen_rect,
                time: Some(time),
                events,
                ..Default::default()
            },
        );
        if let Some(update) = output.platform_output.accesskit_update
            && let Some((_, node)) = update
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some("Remove"))
        {
            last_remove = node.bounds();
        }
    }
    let remove = last_remove.expect("recent workspace action");
    assert!(
        remove.y0 > 84.0 && remove.y1 <= 210.0,
        "recent action did not scroll into the viewport: {remove:?}"
    );
}

#[test]
fn right_click_on_branch_opens_actions_without_switching() {
    use gitrust_core::{
        navigation::{NavigationView, Reference},
        workspaces::{Activity, ChangeCounts, Checkout, RepositoryView, Workspace},
    };

    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    shell.set_repository_view(RepositoryView {
        git_available: true,
        activity: Activity::Idle,
        active: Some(Workspace {
            path: "/fixture".into(),
            checkout: Checkout::Branch("main".into()),
            has_remote: false,
            changes: Some(ChangeCounts::default()),
        }),
        navigation: Some(NavigationView {
            refs: [Reference {
                name: (&b"refs/heads/topic"[..]).into(),
                label: "topic".into(),
                commit: "1".repeat(40),
                upstream: None,
                divergence: None,
                remote: false,
                current: false,
                occupied: false,
            }]
            .into(),
            ..Default::default()
        }),
        ..Default::default()
    });
    shell.open_popup = Some(commands::BRANCH_POPUP);
    let _ = render(&ctx, &mut shell, input(0.0));
    let target = |shell: &mut Desktop, time, label: &str| {
        ctx.disable_accesskit();
        ctx.enable_accesskit();
        let tree = render(&ctx, shell, input(time))
            .platform_output
            .accesskit_update
            .expect("accessibility tree");
        let bounds = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
            })
            .expect(label);
        #[allow(clippy::cast_possible_truncation)]
        Pos2::new(
            f64::midpoint(bounds.x0, bounds.x1) as f32,
            f64::midpoint(bounds.y0, bounds.y1) as f32,
        )
    };
    let click = |shell: &mut Desktop, pos, button, time| {
        let _ = render(
            &ctx,
            shell,
            RawInput {
                events: vec![Event::PointerMoved(pos)],
                ..input(time)
            },
        );
        for (pressed, offset) in [(true, 0.01), (false, 0.02)] {
            let output = render(
                &ctx,
                shell,
                RawInput {
                    events: vec![Event::PointerButton {
                        pos,
                        pressed,
                        button,
                        modifiers: Modifiers::NONE,
                    }],
                    ..input(time + offset)
                },
            );
            if !pressed {
                return output.platform_output.commands;
            }
        }
        unreachable!()
    };
    let row = target(&mut shell, 0.1, "topic");
    let _ = click(&mut shell, row, egui::PointerButton::Secondary, 0.2);
    assert!(shell.take_repository_action().is_none());
    let _ = target(&mut shell, 0.3, "Rename…");
    let _ = target(&mut shell, 0.4, "Delete…");
    let copy = target(&mut shell, 0.5, "Copy branch name");
    let commands = click(&mut shell, copy, egui::PointerButton::Primary, 0.6);
    assert!(commands.contains(&egui::OutputCommand::CopyText("topic".into())));
    assert!(shell.take_repository_action().is_none());
}

#[test]
fn workspace_hides_success_notices_and_keeps_failures_below_tabs() {
    use gitrust_core::{
        changes::ChangesView,
        navigation::{NavigationNotice, NavigationView},
        remotes::{RemoteNotice, RemotesView},
        workspaces::{Activity, ChangeCounts, Checkout, RepositoryView, Workspace},
    };

    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    let mut view = RepositoryView {
        git_available: true,
        activity: Activity::Idle,
        active: Some(Workspace {
            path: "/fixture".into(),
            checkout: Checkout::Branch("main".into()),
            has_remote: true,
            changes: Some(ChangeCounts::default()),
        }),
        changes: Some(ChangesView::default()),
        navigation: Some(NavigationView {
            notice: Some(NavigationNotice::BranchUpdated),
            ..Default::default()
        }),
        remotes: Some(RemotesView {
            notice: Some(RemoteNotice::Pushed),
            ..Default::default()
        }),
        ..Default::default()
    };
    for (time, failure) in [(0.0, false), (0.1, true)] {
        if failure {
            view.remotes.as_mut().expect("remotes").notice = Some(RemoteNotice::Offline);
        }
        shell.set_repository_view(view.clone());
        ctx.disable_accesskit();
        ctx.enable_accesskit();
        let tree = render(&ctx, &mut shell, input(time))
            .platform_output
            .accesskit_update
            .expect("accessibility tree");
        let bounds = |label| {
            tree.nodes.iter().find_map(|(_, node)| {
                (node.label() == Some(label) || node.value() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
            })
        };
        assert!(bounds("Push completed.").is_none());
        assert!(bounds("Branch updated.").is_none());
        let changes = bounds("Changes").expect("Changes tab");
        if failure {
            let remote_error = remotes::notice_text(RemoteNotice::Offline, Language::English);
            assert!(bounds(remote_error).expect("remote error").y0 > changes.y1);
        }
    }
}

#[test]
fn escape_returns_to_workspace_from_github() {
    let ctx = Context::default();
    let mut shell = Desktop::new(&ctx, UiOptions::default());
    shell.enable_repository_management();
    shell.screen = Screen::Github;
    let _ = render(&ctx, &mut shell, input(0.0));
    let _ = render(
        &ctx,
        &mut shell,
        RawInput {
            events: vec![key(Key::Escape, Modifiers::NONE)],
            ..input(0.1)
        },
    );
    assert_eq!(shell.screen, Screen::Workspace);
}
