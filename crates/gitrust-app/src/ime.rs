use eframe::egui::{Context, Event, Id, ImeEvent, output::IMEOutput};
use std::sync::Arc;

pub(crate) struct WaylandIme {
    last_output: Option<IMEOutput>,
    ui_pass: Id,
}

impl WaylandIme {
    pub(crate) fn install(ctx: &Context) -> Self {
        let ui_pass = Id::unique("gitRUST Wayland IME UI pass");
        ctx.on_begin_pass(
            "gitRUST Wayland IME UI pass",
            Arc::new(move |ctx| {
                let key = ui_pass.with(ctx.viewport_id());
                ctx.data_mut(|data| data.insert_temp(key, true));
            }),
        );
        ctx.on_end_pass(
            "gitRUST Wayland IME cursor area",
            Arc::new(move |ctx| {
                let key = ui_pass.with(ctx.viewport_id());
                ctx.data_mut(|data| data.insert_temp(key, false));
                // A cursor-area update can echo the current preedit. Let widgets
                // handle it before removing it from egui-winit's resend check.
                // A changed rectangle, commits and native focus still reach the
                // integration. Remove this filter when adopting egui PR #8906.
                ctx.input_mut(|input| {
                    input
                        .events
                        .retain(|event| !matches!(event, Event::Ime(ImeEvent::Preedit { .. })));
                });
            }),
        );
        Self {
            last_output: None,
            ui_pass,
        }
    }

    pub(crate) fn remember_ui_output(&mut self, ctx: &Context) {
        self.last_output = ctx.output(|output| output.ime);
    }

    pub(crate) fn preserve_during_logic(&self, ctx: &Context) {
        let key = self.ui_pass.with(ctx.viewport_id());
        let has_ui_pass = ctx.data(|data| data.get_temp::<bool>(key).unwrap_or(false));
        let window_focused =
            ctx.input(|input| input.raw.focused && input.raw.viewport().minimized != Some(true));
        if !has_ui_pass
            && window_focused
            && let Some(ime) = self.last_output
        {
            // When a redraw is overdue, eframe 0.37 runs logic without drawing
            // widgets and forwards the output to egui-winit. Keep the focused
            // editor's IME active until the next real UI pass.
            ctx.output_mut(|output| output.ime = Some(ime));
            // run_logic exposes the previous pass's processed events. The
            // integration queues new RawInput for the next UI pass separately.
            ctx.input_mut(|input| input.events.clear());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Id, RawInput, TextEdit};

    fn preedit(text: &str) -> Event {
        Event::Ime(ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: None,
        })
    }

    fn ranged_preedit(text: &str, active_range_chars: std::ops::Range<usize>) -> Event {
        Event::Ime(ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: Some(active_range_chars),
        })
    }

    #[test]
    fn prevents_preedit_feedback_after_widgets_without_removing_commits_or_focus() {
        let ctx = Context::default();
        let _ime = WaylandIme::install(&ctx);
        let meaningful = [
            Event::WindowFocused(true),
            Event::Ime(ImeEvent::Commit("候補".into())),
            Event::Ime(ImeEvent::Commit(String::new())),
        ];
        let mut events = vec![
            preedit(""),
            ranged_preedit("", 0..0),
            preedit("候補"),
            ranged_preedit("候補", 0..1),
        ];
        events.extend(meaningful.iter().cloned());
        let input = RawInput {
            events: events.clone(),
            ..Default::default()
        };
        let mut visible_to_ui = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            visible_to_ui.clone_from(&ui.input(|i| i.events.clone()));
        });
        output.textures_delta.clear();
        assert_eq!(visible_to_ui, events);
        assert_eq!(ctx.input(|i| i.events.clone()), meaningful);
    }

    #[test]
    fn text_edit_can_clear_final_preedit_and_commit_unicode() {
        let ctx = Context::default();
        let _ime = WaylandIme::install(&ctx);
        let id = Id::unique("composition test");
        let mut text = String::new();
        let mut frame = |events| {
            let input = RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ui.add(TextEdit::singleline(&mut text).id(id));
            });
            output.textures_delta.clear();
            text.clone()
        };

        let _ = frame(Vec::new());
        ctx.memory_mut(|memory| memory.request_focus(id));
        let _ = frame(Vec::new());
        assert_eq!(frame(vec![ranged_preedit("á", 0..1)]), "á");
        assert_eq!(frame(vec![ranged_preedit("", 0..0)]), "");
        assert!(ctx.input(|i| i.events.is_empty()));
        assert_eq!(frame(vec![preedit("候")]), "候");
        assert_eq!(
            frame(vec![
                preedit(""),
                Event::Ime(ImeEvent::Commit("候補".into()))
            ]),
            "候補"
        );
        assert_eq!(frame(vec![preedit("")]), "候補");
        assert_eq!(frame(vec![preedit("漢")]), "候補漢");
        assert_eq!(
            frame(vec![Event::Ime(ImeEvent::Commit(String::new()))]),
            "候補"
        );
    }

    #[test]
    fn missed_redraw_keeps_a_focused_editor_active_without_replaying_input() {
        let ctx = Context::default();
        let id = Id::unique("missed redraw editor");
        let mut ime = WaylandIme::install(&ctx);
        let mut text = String::new();
        let mut initial = ctx.run_ui(RawInput::default(), |ui| {
            ui.add(TextEdit::singleline(&mut text).id(id));
        });
        initial.textures_delta.clear();
        ctx.memory_mut(|memory| memory.request_focus(id));
        let input = RawInput {
            events: vec![Event::Text("a".into())],
            ..Default::default()
        };
        let mut frame = ctx.run_ui(input, |ui| {
            ime.preserve_during_logic(ui.ctx());
            assert!(ui.ctx().output(|output| output.ime.is_none()));
            ui.add(TextEdit::singleline(&mut text).id(id));
            ime.remember_ui_output(ui.ctx());
        });
        frame.textures_delta.clear();
        let original = frame.platform_output.ime.expect("focused editor IME");
        let input = RawInput::default();
        let fallback = ctx.run_logic(&input, |ctx| ime.preserve_during_logic(ctx));
        assert_eq!(
            fallback
                .platform_output
                .ime
                .expect("preserved IME")
                .cursor_rect,
            original.cursor_rect
        );
        assert!(ctx.input(|input| input.events.is_empty()));

        let unfocused = RawInput {
            focused: false,
            ..input
        };
        let fallback = ctx.run_logic(&unfocused, |ctx| ime.preserve_during_logic(ctx));
        assert!(fallback.platform_output.ime.is_none());
    }
}
