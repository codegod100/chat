//! Themed CommonMark rendering for assistant messages (incl. LaTeX math).

use std::cell::RefCell;
use std::rc::Rc;

use egui::Ui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use vidya::Theme;

use crate::math::{self, MathCache};

pub struct MarkdownView {
    cache: CommonMarkCache,
    math: Rc<RefCell<MathCache>>,
}

impl Default for MarkdownView {
    fn default() -> Self {
        Self {
            cache: CommonMarkCache::default(),
            math: Rc::new(RefCell::new(MathCache::default())),
        }
    }
}

impl MarkdownView {
    pub fn show(&mut self, ui: &mut Ui, _theme: &Theme, markdown: &str) {
        ui.style_mut().url_in_tooltip = true;
        // Vidya forces Wrap globally so chat chrome hugs the window edge. That
        // breaks egui_commonmark tables: Grid cells get a tiny provisional
        // width, labels wrap to it, and the column locks narrow (mid-word
        // breaks). Clear the override so layout decides — wrapping horizontal
        // for body text, Extend inside Grid cells.
        ui.style_mut().wrap_mode = None;
        let normalized = math::normalize_math_delimiters(markdown);
        let math = Rc::clone(&self.math);
        let render_math = move |ui: &mut Ui, tex: &str, inline: bool| {
            math.borrow_mut().show(ui, tex, inline);
        };
        CommonMarkViewer::new()
            .max_image_width(Some(ui.available_width() as usize))
            .render_math_fn(Some(&render_math))
            .show(ui, &mut self.cache, &normalized);
    }
}
