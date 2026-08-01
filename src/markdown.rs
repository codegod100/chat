//! Themed CommonMark rendering for assistant messages.

use egui::Ui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use vidya::Theme;

pub struct MarkdownView {
    cache: CommonMarkCache,
}

impl Default for MarkdownView {
    fn default() -> Self {
        Self {
            cache: CommonMarkCache::default(),
        }
    }
}

impl MarkdownView {
    pub fn show(&mut self, ui: &mut Ui, _theme: &Theme, markdown: &str) {
        ui.style_mut().url_in_tooltip = true;
        CommonMarkViewer::new()
            .max_image_width(Some(ui.available_width() as usize))
            .show(ui, &mut self.cache, markdown);
    }
}
