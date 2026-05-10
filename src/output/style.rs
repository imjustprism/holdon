use std::borrow::Cow;

use owo_colors::OwoColorize;

use super::theme::Color;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Style {
    color: bool,
}

impl Style {
    pub(crate) const fn new(color: bool) -> Self {
        Self { color }
    }

    pub(crate) fn paint(self, text: &str, color: Color) -> Cow<'_, str> {
        if !self.color {
            return Cow::Borrowed(text);
        }
        let (r, g, b) = color.rgb();
        let painted = text.truecolor(r, g, b);
        Cow::Owned(if color.bold() {
            painted.bold().to_string()
        } else {
            painted.to_string()
        })
    }
}
