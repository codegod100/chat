//! Chat window: header, messages, compose.

use std::collections::BTreeMap;

use eframe::egui::{self, Align, Event, Key, Layout, RichText, ScrollArea, TextEdit};
use vidya::{
    apply, body, button, destructive_button, dialog, dim_label, primary_button,
    reserve_system_chrome, text_field_multiline, text_field_singleline, title, Theme, TypeScale,
};

use crate::bao;
use crate::chat::{self, ChatEvent, Message, Role, StreamHandle};
use crate::markdown::MarkdownView;
use crate::providers::{self, ModelChoice, Provider};
use crate::session::{self, SavedSession};

struct ResumeDialog {
    sessions: Vec<SavedSession>,
    filter: String,
    selected: Option<String>,
    load_error: Option<String>,
}

impl ResumeDialog {
    fn open() -> Self {
        let (sessions, load_error) = match session::list_sessions() {
            Ok(s) => (s, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        Self {
            sessions,
            filter: String::new(),
            selected: None,
            load_error,
        }
    }

    fn filtered(&self) -> Vec<&SavedSession> {
        let q = self.filter.trim().to_lowercase();
        self.sessions
            .iter()
            .filter(|s| {
                if q.is_empty() {
                    return true;
                }
                s.title.to_lowercase().contains(&q)
                    || s.model_id.to_lowercase().contains(&q)
                    || s.provider_id.to_lowercase().contains(&q)
            })
            .collect()
    }
}

pub struct ChatApp {
    md: MarkdownView,

    providers: Vec<Provider>,
    provider_idx: usize,
    models: Vec<ModelChoice>,
    model_id: String,

    messages: Vec<Message>,
    draft: String,
    status: String,
    keys_ok: bool,
    /// Shown when OpenBao has no stored token (common on Android).
    need_token: bool,
    token_draft: String,

    stream: Option<StreamHandle>,
    streaming: bool,
    scroll_follow: bool,
    compose_focused: bool,
    focus_compose_once: bool,

    /// Inline edit of a prior user message; resubmit truncates the thread.
    editing_idx: Option<usize>,
    edit_draft: String,

    /// Defer blocking OpenBao / model fetches off mid-layout.
    pending_load: bool,
    pending_models: bool,

    /// Current saved session; `None` until the first user message.
    session_id: Option<String>,
    resume_dialog: Option<ResumeDialog>,
    /// Restore provider after keys load (from a resumed session).
    pending_provider_id: Option<String>,
    /// Restore model after the model list loads.
    pending_model_id: Option<String>,
}

/// Larger than Vidya defaults — chat body text needs to be easy to read.
fn chat_theme() -> Theme {
    let mut th = Theme::dark();
    th.type_scale = TypeScale {
        title: 24.0,
        title_2: 18.0,
        title_3: 16.0,
        body: 16.0,
        caption: 14.0,
    };
    th
}

impl ChatApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply(&cc.egui_ctx, &chat_theme());

        Self {
            md: MarkdownView::default(),
            providers: Vec::new(),
            provider_idx: 0,
            models: Vec::new(),
            model_id: String::new(),
            messages: Vec::new(),
            draft: String::new(),
            status: "Loading keys from OpenBao…".into(),
            keys_ok: false,
            need_token: false,
            token_draft: String::new(),
            stream: None,
            streaming: false,
            scroll_follow: true,
            compose_focused: false,
            focus_compose_once: true,
            editing_idx: None,
            edit_draft: String::new(),
            pending_load: true,
            pending_models: false,
            session_id: None,
            resume_dialog: None,
            pending_provider_id: None,
            pending_model_id: None,
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
                self.need_token = matches!(e, bao::BaoError::NoToken);
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
            self.need_token = false;
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
        if let Some(want) = self.pending_provider_id.take() {
            if let Some(i) = self.providers.iter().position(|p| p.id == want) {
                self.provider_idx = i;
            }
        }
        self.keys_ok = true;
        self.need_token = false;
        self.status = format!("Loaded {n} provider{}", if n == 1 { "" } else { "s" });
        self.pending_models = true;
    }

    fn apply_token_draft(&mut self) {
        let t = self.token_draft.trim().to_string();
        if t.is_empty() {
            self.status = "Paste an OpenBao token first.".into();
            return;
        }
        bao::set_token_override(t);
        self.token_draft.clear();
        self.pending_load = true;
    }

    fn load_models_for_current(&mut self) {
        let Some(provider) = self.current_provider().cloned() else {
            return;
        };
        self.status = format!("Fetching models ({})…", provider.label);
        match providers::list_models(&provider) {
            Ok(models) => {
                let preferred = self
                    .pending_model_id
                    .take()
                    .filter(|id| models.iter().any(|m| m.id == *id));
                self.model_id = preferred
                    .unwrap_or_else(|| providers::pick_default_model(&provider, &models));
                self.models = models;
                self.status = format!("{} · {}", provider.label, self.model_id);
            }
            Err(e) => {
                let fallback = self
                    .pending_model_id
                    .take()
                    .unwrap_or_else(|| provider.default_model.clone());
                self.models = vec![ModelChoice {
                    id: fallback.clone(),
                    label: fallback.clone(),
                }];
                self.model_id = fallback;
                self.status = format!("{} (models: {e})", provider.label);
            }
        }
    }

    fn persist_current(&mut self) {
        if self.messages.is_empty() {
            return;
        }
        let id = self
            .session_id
            .get_or_insert_with(session::new_session_id)
            .clone();
        let provider_id = self
            .current_provider()
            .map(|p| p.id.to_string())
            .unwrap_or_default();
        let session = SavedSession {
            id,
            title: session::title_from_messages(&self.messages),
            updated_at_ms: session::now_ms(),
            provider_id,
            model_id: self.model_id.clone(),
            messages: self.messages.clone(),
        };
        if let Err(e) = session::save_session(&session) {
            self.status = format!("Save failed: {e}");
        }
    }

    fn cancel_stream(&mut self) {
        if let Some(h) = self.stream.take() {
            h.cancel();
        }
        self.streaming = false;
    }

    fn clear_editing(&mut self) {
        self.editing_idx = None;
        self.edit_draft.clear();
    }

    /// Stop an in-flight completion from the UI (keeps any tokens already received).
    fn stop_generation(&mut self) {
        if !self.streaming {
            return;
        }
        self.cancel_stream();
        if self
            .messages
            .last()
            .is_some_and(|m| m.role == Role::Assistant && m.content.is_empty())
        {
            self.messages.pop();
        }
        self.persist_current();
        if let Some(p) = self.current_provider() {
            self.status = format!("Stopped · {} · {}", p.label, self.model_id);
        } else {
            self.status = "Stopped".into();
        }
    }

    fn new_chat(&mut self) {
        self.cancel_stream();
        self.clear_editing();
        self.persist_current();
        self.messages.clear();
        self.draft.clear();
        self.session_id = None;
        self.scroll_follow = true;
        if let Some(p) = self.current_provider() {
            self.status = format!("{} · {}", p.label, self.model_id);
        }
    }

    fn resume_session(&mut self, id: &str) {
        self.cancel_stream();
        self.clear_editing();
        if self.session_id.as_deref() != Some(id) {
            self.persist_current();
        }
        match session::load_session(id) {
            Ok(session) => {
                self.session_id = Some(session.id);
                self.messages = session.messages;
                self.draft.clear();
                self.scroll_follow = true;
                self.pending_provider_id = Some(session.provider_id);
                self.pending_model_id = Some(session.model_id);
                if self.keys_ok && !self.providers.is_empty() {
                    if let Some(want) = self.pending_provider_id.take() {
                        if let Some(i) = self.providers.iter().position(|p| p.id == want) {
                            self.provider_idx = i;
                        }
                    }
                    self.pending_models = true;
                }
                self.status = format!("Resumed · {}", session::title_from_messages(&self.messages));
            }
            Err(e) => {
                self.status = format!("Resume failed: {e}");
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
            self.persist_current();
            return;
        }

        if done {
            self.streaming = false;
            self.stream = None;
            self.persist_current();
            if let Some(p) = self.current_provider() {
                self.status = format!("{} · {}", p.label, self.model_id);
            }
            return;
        }

        if self.streaming {
            ctx.request_repaint();
        }
    }

    fn begin_stream(&mut self) {
        let Some(provider) = self.current_provider().cloned() else {
            self.status = "No provider available.".into();
            self.streaming = false;
            return;
        };
        if self.model_id.is_empty() {
            self.status = "No model selected.".into();
            self.streaming = false;
            return;
        }

        if self.session_id.is_none() {
            self.session_id = Some(session::new_session_id());
        }

        self.clear_editing();
        self.scroll_follow = true;
        self.streaming = true;
        self.status = format!("Thinking… ({})", self.model_id);

        let mut req_history = self.messages.clone();
        // Drop the empty assistant stub from the request history.
        if req_history
            .last()
            .is_some_and(|m| m.role == Role::Assistant && m.content.is_empty())
        {
            req_history.pop();
        }

        // Persist user turn immediately so a crash mid-stream still resumes.
        self.persist_current();

        self.stream = Some(chat::start_stream(
            provider,
            self.model_id.clone(),
            req_history,
        ));
    }

    fn send(&mut self) {
        if self.streaming {
            return;
        }
        let text = self.draft.trim().to_string();
        if text.is_empty() {
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
        self.begin_stream();
    }

    /// Replace a user message, drop everything after it, and regenerate.
    fn resubmit_edit(&mut self) {
        if self.streaming {
            return;
        }
        let Some(idx) = self.editing_idx else {
            return;
        };
        if idx >= self.messages.len() || self.messages[idx].role != Role::User {
            self.clear_editing();
            return;
        }
        let text = self.edit_draft.trim().to_string();
        if text.is_empty() {
            return;
        }

        self.messages.truncate(idx);
        self.messages.push(Message {
            role: Role::User,
            content: text,
        });
        self.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
        });
        self.begin_stream();
    }

    fn clear_chat(&mut self) {
        self.cancel_stream();
        self.clear_editing();
        self.messages.clear();
        self.draft.clear();
        self.session_id = None;
        if let Some(p) = self.current_provider() {
            self.status = format!("{} · {}", p.label, self.model_id);
        }
    }

    fn on_provider_changed(&mut self) {
        self.cancel_stream();
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

        let th = chat_theme();
        apply(ctx, &th);
        // Edge-to-edge NativeActivity: keep chrome clear of status / gesture bars.
        reserve_system_chrome(ctx, &th);

        // Enter sends (Shift+Enter = newline). Consume bare Enter before TextEdit.
        let mut send_chord = false;
        if self.compose_focused && !self.streaming && self.keys_ok && self.editing_idx.is_none() {
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

        // Escape: stop generation, or abandon an inline edit.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.streaming {
                self.stop_generation();
            } else if self.editing_idx.is_some() {
                self.clear_editing();
            }
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
                    .stroke(egui::Stroke::new(1.0_f32, th.palette.border_soft)),
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

        self.show_resume_dialog(ctx, &th);
    }
}

impl ChatApp {
    fn ui_header(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let mut refresh = false;
        let mut clear = false;
        let mut new_chat = false;
        let mut open_resume = false;
        let mut provider_changed = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = th.spacing.sm;

            title(ui, th, "Chat");

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
                if button(ui, th, "Clear").clicked() {
                    clear = true;
                }
                if button(ui, th, "New").clicked() {
                    new_chat = true;
                }
                if button(ui, th, "Resume").clicked() {
                    open_resume = true;
                }
                if button(ui, th, "Refresh").clicked() {
                    refresh = true;
                }
            });
        });

        ui.add_space(th.spacing.xs);
        dim_label(ui, th, &self.status);

        if self.need_token {
            ui.add_space(th.spacing.sm);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.token_draft)
                        .hint_text("OpenBao token")
                        .password(true)
                        .desired_width(ui.available_width().min(320.0)),
                );
                if primary_button(ui, th, "Use token").clicked() {
                    self.apply_token_draft();
                }
            });
        }

        if clear {
            self.clear_chat();
        }
        if new_chat {
            self.new_chat();
        }
        if open_resume {
            self.resume_dialog = Some(ResumeDialog::open());
        }
        if refresh {
            self.cancel_stream();
            self.pending_load = true;
        }
        if provider_changed {
            self.on_provider_changed();
        }
    }

    fn show_resume_dialog(&mut self, ctx: &egui::Context, th: &Theme) {
        let Some(mut dlg) = self.resume_dialog.take() else {
            return;
        };

        let mut resume = false;
        let mut cancel = false;
        let mut refresh = false;
        let mut keep_open = true;
        let filtered: Vec<SavedSession> = dlg.filtered().into_iter().cloned().collect();
        let selected = dlg.selected.clone();

        dialog("Resume session", th)
            .id(egui::Id::new("chat_resume_dialog"))
            .default_size([420.0, 340.0])
            .min_width(300.0)
            .min_height(240.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    dim_label(ui, th, "Filter");
                    ui.add_space(th.spacing.sm);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, th, "↻")
                            .on_hover_text("Refresh")
                            .clicked()
                        {
                            refresh = true;
                        }
                        let _ = text_field_singleline(ui, th, &mut dlg.filter);
                    });
                });
                ui.add_space(th.spacing.xs);

                if let Some(err) = &dlg.load_error {
                    ui.colored_label(th.palette.destructive, err);
                    ui.add_space(th.spacing.xs);
                }

                let footer_reserve = th.spacing.control_height * 2.0 + th.spacing.sm * 3.0;
                let list_h = (ui.available_height() - footer_reserve).max(80.0);
                ScrollArea::vertical()
                    .max_height(list_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            dim_label(ui, th, "No saved chats yet.");
                            return;
                        }
                        for chat in &filtered {
                            let is_sel = selected.as_deref() == Some(chat.id.as_str());
                            let title = RichText::new(&chat.title)
                                .size(th.type_scale.body)
                                .color(if is_sel {
                                    th.palette.accent_fg
                                } else {
                                    th.palette.text
                                });
                            let meta = format!(
                                "{} · {} · {}",
                                chat.provider_id,
                                chat.model_id,
                                chat.age_label(),
                            );
                            let fill = if is_sel {
                                th.palette.accent
                            } else {
                                th.palette.popover_bg
                            };
                            let stroke = egui::Stroke::new(
                                1.0_f32,
                                if is_sel {
                                    th.palette.accent
                                } else {
                                    th.palette.border_soft
                                },
                            );
                            let row = egui::Frame::NONE
                                .fill(fill)
                                .stroke(stroke)
                                .corner_radius(th.spacing.radius_sm)
                                .inner_margin(egui::Margin::symmetric(
                                    th.spacing.sm as i8,
                                    th.spacing.xs as i8,
                                ))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(title);
                                    ui.label(
                                        RichText::new(meta)
                                            .size(th.type_scale.caption)
                                            .color(if is_sel {
                                                th.palette.accent_fg
                                            } else {
                                                th.palette.text_secondary
                                            }),
                                    );
                                });
                            let resp = row.response.interact(egui::Sense::click());
                            if resp.clicked() {
                                dlg.selected = Some(chat.id.clone());
                            }
                            if resp.double_clicked() {
                                dlg.selected = Some(chat.id.clone());
                                resume = true;
                            }
                            ui.add_space(th.spacing.xs);
                        }
                    });

                ui.add_space(th.spacing.sm);
                ui.horizontal(|ui| {
                    let can_resume = dlg.selected.is_some();
                    ui.add_enabled_ui(can_resume, |ui| {
                        if primary_button(ui, th, "Resume").clicked() {
                            resume = true;
                        }
                    });
                    if button(ui, th, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel {
            keep_open = false;
        }
        if refresh {
            self.resume_dialog = Some(ResumeDialog::open());
            return;
        }
        if resume {
            if let Some(id) = dlg.selected.clone() {
                self.resume_session(&id);
                keep_open = false;
            }
        }
        if keep_open {
            self.resume_dialog = Some(dlg);
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

        let mut start_edit: Option<usize> = None;
        let mut cancel_edit = false;
        let mut do_resubmit = false;

        let output = scroll.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let max_w = ui.available_width();
            let streaming = self.streaming;
            let n = self.messages.len();
            let can_edit = !streaming && self.keys_ok;

            for i in 0..n {
                // Unique id namespace per message so egui_commonmark tables
                // (Grid::new(ui.id().with("_table").with(n))) don't clash.
                ui.push_id(i, |ui| {
                    let is_last = i + 1 == n;
                    let role = self.messages[i].role;
                    let content = self.messages[i].content.clone();
                    let is_editing = self.editing_idx == Some(i);
                    ui.add_space(th.spacing.sm);

                    match role {
                        Role::User => {
                            egui::Frame::NONE
                                .fill(th.palette.card_bg)
                                .stroke(egui::Stroke::new(1.0_f32, th.palette.border_soft))
                                .corner_radius(th.spacing.radius_md)
                                .inner_margin(egui::Margin::symmetric(
                                    th.spacing.md as i8,
                                    th.spacing.sm as i8,
                                ))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("You")
                                                .size(th.type_scale.caption)
                                                .color(th.palette.text_secondary),
                                        );
                                        if can_edit && !is_editing {
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if button(ui, th, "Edit")
                                                        .on_hover_text("Edit and resubmit")
                                                        .clicked()
                                                    {
                                                        start_edit = Some(i);
                                                    }
                                                },
                                            );
                                        }
                                    });
                                    ui.add_space(2.0);
                                    if is_editing {
                                        let _ = text_field_multiline(ui, th, &mut self.edit_draft, 4);
                                        ui.add_space(th.spacing.xs);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = th.spacing.sm;
                                            ui.add_enabled_ui(
                                                !self.edit_draft.trim().is_empty(),
                                                |ui| {
                                                    if primary_button(ui, th, "Resubmit").clicked()
                                                    {
                                                        do_resubmit = true;
                                                    }
                                                },
                                            );
                                            if button(ui, th, "Cancel").clicked() {
                                                cancel_edit = true;
                                            }
                                        });
                                    } else {
                                        body(ui, th, &content);
                                    }
                                });
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
                });
            }
        });

        if let Some(i) = start_edit {
            if i < self.messages.len() && self.messages[i].role == Role::User {
                self.editing_idx = Some(i);
                self.edit_draft = self.messages[i].content.clone();
            }
        }
        if cancel_edit {
            self.clear_editing();
        }
        if do_resubmit {
            self.resubmit_edit();
        }

        let scrolled = ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.5);
        if scrolled {
            let at_bottom = output.state.offset.y + output.inner_rect.height() + 48.0
                >= output.content_size.y;
            self.scroll_follow = at_bottom;
        }
    }

    fn ui_compose(&mut self, ui: &mut egui::Ui, th: &Theme, send_chord: bool) {
        let mut do_send = send_chord;
        let mut do_stop = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = th.spacing.sm;

            let trail_label = if self.streaming { "Stop" } else { "Send" };
            let trail_w = compose_action_width(ui, th, trail_label);
            let trail_gutter = th.spacing.sm;
            let field_w =
                (ui.available_width() - trail_w - th.spacing.sm - trail_gutter).max(1.0);

            ui.allocate_ui_with_layout(
                egui::vec2(field_w, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.set_width(field_w);
                    ui.set_max_width(field_w);
                    let resp = text_field_multiline(ui, th, &mut self.draft, 3);
                    self.compose_focused = resp.has_focus();
                    if self.focus_compose_once && self.keys_ok {
                        resp.request_focus();
                        self.focus_compose_once = false;
                    }
                },
            );

            if self.streaming {
                if destructive_button(ui, th, "Stop")
                    .on_hover_text("Stop generating (Esc)")
                    .clicked()
                {
                    do_stop = true;
                }
            } else {
                ui.add_enabled_ui(self.keys_ok && self.editing_idx.is_none(), |ui| {
                    if primary_button(ui, th, "Send").clicked() {
                        do_send = true;
                    }
                });
            }
            ui.add_space(trail_gutter);
        });

        if do_stop {
            self.stop_generation();
        }
        if do_send {
            self.send();
        }
    }
}

fn compose_action_width(ui: &egui::Ui, th: &Theme, label: &str) -> f32 {
    let pad_x = th.spacing.lg;
    let galley = ui.fonts(|fonts| {
        fonts.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(th.type_scale.body),
            th.palette.accent_fg,
        )
    });
    galley.size().x + pad_x * 2.0
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}
