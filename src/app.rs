//! Chat window: header, messages, compose.

use std::collections::BTreeMap;

use eframe::egui::{self, Align, Event, Key, Layout, RichText, ScrollArea};
use vidya::{
    apply, body, button, dim_label, lead_trail, primary_button, status_dot,
    text_field_multiline, title, Mode, Theme,
};

use crate::bao;
use crate::chat::{self, ChatEvent, Message, Role, StreamHandle};
use crate::markdown::MarkdownView;
use crate::providers::{self, ModelChoice, Provider};

pub struct ChatApp {
    mode: Mode,
    md: MarkdownView,

    providers: Vec<Provider>,
    provider_idx: usize,
    models: Vec<ModelChoice>,
    model_id: String,

    messages: Vec<Message>,
    draft: String,
    status: String,
    keys_ok: bool,

    stream: Option<StreamHandle>,
    streaming: bool,
    scroll_follow: bool,
    compose_focused: bool,

    /// Defer blocking OpenBao / model fetches off mid-layout.
    pending_load: bool,
    pending_models: bool,
}

impl ChatApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme = Theme::dark();
        apply(&cc.egui_ctx, &theme);

        Self {
            mode: Mode::Dark,
            md: MarkdownView::default(),
            providers: Vec::new(),
            provider_idx: 0,
            models: Vec::new(),
            model_id: String::new(),
            messages: Vec::new(),
            draft: String::new(),
            status: "Loading keys from OpenBao…".into(),
            keys_ok: false,
            stream: None,
            streaming: false,
            scroll_follow: true,
            compose_focused: false,
            pending_load: true,
            pending_models: false,
        }
    }

    fn theme(&self) -> Theme {
        match self.mode {
            Mode::Dark => Theme::dark(),
            Mode::Light => Theme::light(),
        }
    }

    fn current_provider(&self) -> Option<&Provider> {
        self.providers.get(self.provider_idx)
    }

    fn load_keys(&mut self) {
        self.status = "Loading keys from OpenBao…".into();
        match bao::fetch_ai_keys() {
            Ok(keys) => self.apply_keys(keys),
            Err(e) => {
                self.keys_ok = false;
                self.providers.clear();
                self.models.clear();
                self.model_id.clear();
                self.status = e.to_string();
            }
        }
    }

    fn apply_keys(&mut self, keys: BTreeMap<String, String>) {
        let providers = providers::from_keys(&keys);
        if providers.is_empty() {
            self.keys_ok = false;
            self.providers.clear();
            self.models.clear();
            self.model_id.clear();
            self.status =
                "No known LLM keys in secret/ai-api-keys (need OPENROUTER_, DEEPSEEK_, …)".into();
            return;
        }

        let n = providers.len();
        self.providers = providers;
        self.provider_idx = 0;
        self.keys_ok = true;
        self.status = format!("Loaded {n} provider{}", if n == 1 { "" } else { "s" });
        self.pending_models = true;
    }

    fn load_models_for_current(&mut self) {
        let Some(provider) = self.current_provider().cloned() else {
            return;
        };
        self.status = format!("Fetching models ({})…", provider.label);
        match providers::list_models(&provider) {
            Ok(models) => {
                self.model_id = providers::pick_default_model(&provider, &models);
                self.models = models;
                self.status = format!("{} · {}", provider.label, self.model_id);
            }
            Err(e) => {
                self.models = vec![ModelChoice {
                    id: provider.default_model.clone(),
                    label: provider.default_model.clone(),
                }];
                self.model_id = provider.default_model.clone();
                self.status = format!("{} (models: {e})", provider.label);
            }
        }
    }

    fn poll_stream(&mut self, ctx: &egui::Context) {
        let Some(handle) = self.stream.as_ref() else {
            return;
        };

        let mut deltas = Vec::new();
        let mut done = false;
        let mut error: Option<String> = None;

        while let Ok(ev) = handle.rx.try_recv() {
            match ev {
                ChatEvent::Delta(s) => deltas.push(s),
                ChatEvent::Done => done = true,
                ChatEvent::Error(e) => error = Some(e),
            }
        }

        if !deltas.is_empty() {
            if let Some(last) = self.messages.last_mut() {
                if last.role == Role::Assistant {
                    for d in deltas {
                        last.content.push_str(&d);
                    }
                }
            }
            self.scroll_follow = true;
        }

        if let Some(e) = error {
            if let Some(last) = self.messages.last_mut() {
                if last.role == Role::Assistant {
                    if last.content.is_empty() {
                        last.content = format!("*Error:* {e}");
                    } else {
                        last.content.push_str(&format!("\n\n*Error:* {e}"));
                    }
                }
            } else {
                self.status = e;
            }
            self.streaming = false;
            self.stream = None;
            return;
        }

        if done {
            self.streaming = false;
            self.stream = None;
            if let Some(p) = self.current_provider() {
                self.status = format!("{} · {}", p.label, self.model_id);
            }
            return;
        }

        if self.streaming {
            ctx.request_repaint();
        }
    }

    fn send(&mut self) {
        if self.streaming {
            return;
        }
        let text = self.draft.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(provider) = self.current_provider().cloned() else {
            self.status = "No provider available.".into();
            return;
        };
        if self.model_id.is_empty() {
            self.status = "No model selected.".into();
            return;
        }

        self.draft.clear();
        self.messages.push(Message {
            role: Role::User,
            content: text,
        });
        self.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
        });
        self.scroll_follow = true;
        self.streaming = true;
        self.status = format!("Thinking… ({})", self.model_id);

        let history = self.messages.clone();
        // Drop the empty assistant stub from the request history.
        let mut req_history = history;
        if req_history
            .last()
            .is_some_and(|m| m.role == Role::Assistant && m.content.is_empty())
        {
            req_history.pop();
        }

        self.stream = Some(chat::start_stream(
            provider,
            self.model_id.clone(),
            req_history,
        ));
    }

    fn clear_chat(&mut self) {
        if let Some(h) = self.stream.take() {
            h.cancel();
        }
        self.streaming = false;
        self.messages.clear();
        if let Some(p) = self.current_provider() {
            self.status = format!("{} · {}", p.label, self.model_id);
        }
    }

    fn on_provider_changed(&mut self) {
        if let Some(h) = self.stream.take() {
            h.cancel();
        }
        self.streaming = false;
        self.pending_models = true;
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.pending_load {
            self.pending_load = false;
            self.load_keys();
        }
        if self.pending_models {
            self.pending_models = false;
            self.load_models_for_current();
        }

        self.poll_stream(ctx);

        let th = self.theme();
        apply(ctx, &th);

        // Enter sends (Shift+Enter = newline). Consume bare Enter before TextEdit.
        let mut send_chord = false;
        if self.compose_focused && !self.streaming && self.keys_ok {
            ctx.input_mut(|i| {
                if i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl {
                    return;
                }
                let pressed = i.events.iter().any(|e| {
                    matches!(
                        e,
                        Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            ..
                        }
                    )
                });
                if pressed {
                    send_chord = true;
                    i.events.retain(|e| {
                        !matches!(
                            e,
                            Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
            });
        }

        egui::TopBottomPanel::top("header")
            .frame(th.header_frame())
            .show(ctx, |ui| {
                self.ui_header(ui, &th);
            });

        egui::TopBottomPanel::bottom("compose")
            .frame(
                egui::Frame::new()
                    .fill(th.palette.headerbar_bg)
                    .inner_margin(egui::Margin::symmetric(
                        th.spacing.md as i8,
                        th.spacing.sm as i8,
                    ))
                    .stroke(egui::Stroke::new(1.0, th.palette.border_soft)),
            )
            .show(ctx, |ui| {
                self.ui_compose(ui, &th, send_chord);
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(th.palette.view_bg)
                    .inner_margin(egui::Margin::symmetric(th.spacing.lg as i8, th.spacing.md as i8)),
            )
            .show(ctx, |ui| {
                self.ui_messages(ui, &th);
            });
    }
}

impl ChatApp {
    fn ui_header(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let mut refresh = false;
        let mut clear = false;
        let mut toggle_mode = false;
        let mut provider_changed = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = th.spacing.sm;

            title(ui, th, "Chat");
            ui.add_space(th.spacing.sm);
            status_dot(ui, th, self.keys_ok && !self.providers.is_empty());

            ui.add_space(th.spacing.md);

            if !self.providers.is_empty() {
                let prev = self.provider_idx;
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(
                        self.current_provider()
                            .map(|p| p.label)
                            .unwrap_or("Provider"),
                    )
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for (i, p) in self.providers.iter().enumerate() {
                            if ui
                                .selectable_value(&mut self.provider_idx, i, p.label)
                                .clicked()
                            {
                                // selection applied via selectable_value
                            }
                        }
                    });
                if self.provider_idx != prev {
                    provider_changed = true;
                }

                ui.add_space(th.spacing.xs);

                let model_label = if self.model_id.is_empty() {
                    "Model"
                } else {
                    self.model_id.as_str()
                };
                egui::ComboBox::from_id_salt("model")
                    .selected_text(truncate(model_label, 42))
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for m in &self.models {
                            ui.selectable_value(&mut self.model_id, m.id.clone(), &m.label);
                        }
                    });
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, th, if self.mode == Mode::Dark { "Light" } else { "Dark" })
                    .clicked()
                {
                    toggle_mode = true;
                }
                if button(ui, th, "Clear").clicked() {
                    clear = true;
                }
                if button(ui, th, "Refresh").clicked() {
                    refresh = true;
                }
            });
        });

        ui.add_space(th.spacing.xs);
        dim_label(ui, th, &self.status);

        if toggle_mode {
            self.mode = match self.mode {
                Mode::Dark => Mode::Light,
                Mode::Light => Mode::Dark,
            };
        }
        if clear {
            self.clear_chat();
        }
        if refresh {
            if let Some(h) = self.stream.take() {
                h.cancel();
            }
            self.streaming = false;
            self.pending_load = true;
        }
        if provider_changed {
            self.on_provider_changed();
        }
    }

    fn ui_messages(&mut self, ui: &mut egui::Ui, th: &Theme) {
        if self.messages.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                dim_label(ui, th, "Ask anything");
            });
            return;
        }

        let scroll = ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.scroll_follow);

        let output = scroll.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let max_w = ui.available_width();
            let streaming = self.streaming;
            let n = self.messages.len();

            for i in 0..n {
                let is_last = i + 1 == n;
                let role = self.messages[i].role;
                let content = self.messages[i].content.clone();
                ui.add_space(th.spacing.sm);

                match role {
                    Role::User => {
                        ui.label(
                            RichText::new("You")
                                .size(th.type_scale.caption)
                                .color(th.palette.text_secondary),
                        );
                        ui.add_space(2.0);
                        body(ui, th, &content);
                    }
                    Role::Assistant => {
                        ui.label(
                            RichText::new("Assistant")
                                .size(th.type_scale.caption)
                                .color(th.palette.accent),
                        );
                        ui.add_space(2.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(max_w, 0.0),
                            Layout::top_down(Align::LEFT),
                            |ui| {
                                ui.set_max_width(max_w);
                                if content.is_empty() && streaming && is_last {
                                    dim_label(ui, th, "…");
                                } else {
                                    self.md.show(ui, th, &content);
                                }
                            },
                        );
                    }
                }

                ui.add_space(th.spacing.sm);
                ui.separator();
            }
        });

        let scrolled = ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.5);
        if scrolled {
            let at_bottom = output.state.offset.y + output.inner_rect.height() + 48.0
                >= output.content_size.y;
            self.scroll_follow = at_bottom;
        }
    }

    fn ui_compose(&mut self, ui: &mut egui::Ui, th: &Theme, send_chord: bool) {
        let mut do_send = send_chord;

        lead_trail(
            ui,
            |ui| {
                let resp = text_field_multiline(ui, th, &mut self.draft, 3);
                self.compose_focused = resp.has_focus();
                if self.keys_ok && self.messages.is_empty() && !resp.has_focus() {
                    resp.request_focus();
                }
            },
            |ui| {
                ui.add_space(th.spacing.sm);
                ui.add_enabled_ui(!self.streaming && self.keys_ok, |ui| {
                    if primary_button(ui, th, "Send").clicked() {
                        do_send = true;
                    }
                });
            },
        );

        if do_send {
            self.send();
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
