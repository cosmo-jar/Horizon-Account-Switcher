// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use super::{
    canvas::{Rect, UiCanvas, draw_rounded_outline, draw_rounded_rect},
    theme::{LiquidGlassTheme, argb, with_alpha},
};

/// Visual tone variants for liquid-glass buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonTone {
    Primary,
    Secondary,
    Danger,
}

/// Shared UI widget abstraction encapsulating layout bounds, hover detection, and rendering.
pub trait Widget {
    fn bounds(&self) -> Rect;
    fn draw(&self, canvas: &mut dyn UiCanvas, theme: &LiquidGlassTheme, mouse: [f32; 2]);
}

/// Interactive liquid-glass button with specular rim and hover state.
#[derive(Clone, Debug)]
pub struct GlassButtonWidget {
    pub rect: Rect,
    pub label: String,
    pub tone: ButtonTone,
    pub enabled: bool,
}

impl GlassButtonWidget {
    pub fn new(rect: Rect, label: impl Into<String>, tone: ButtonTone) -> Self {
        Self {
            rect,
            label: label.into(),
            tone,
            enabled: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Returns `true` when clickable and pointer rests inside bounds.
    pub fn hit(&self, mouse: [f32; 2]) -> bool {
        self.enabled && self.rect.contains(mouse[0], mouse[1])
    }
}

impl Widget for GlassButtonWidget {
    fn bounds(&self) -> Rect {
        self.rect
    }

    fn draw(&self, canvas: &mut dyn UiCanvas, theme: &LiquidGlassTheme, mouse: [f32; 2]) {
        let hovered = self.hit(mouse);
        let fill = if !self.enabled {
            argb(255, 255, 255, 10)
        } else {
            theme.button_fill(
                self.tone == ButtonTone::Primary,
                hovered,
                self.tone == ButtonTone::Danger,
            )
        };
        draw_rounded_rect(canvas, self.rect, 7.0, fill);
        let rim = match (self.enabled, self.tone, hovered) {
            (false, _, _) => argb(255, 255, 255, 18),
            (true, ButtonTone::Danger, _) => with_alpha(theme.danger_coral(), 135),
            (true, ButtonTone::Primary, true) => argb(255, 255, 255, 160),
            (true, ButtonTone::Primary, false) => with_alpha(theme.accent_soft(), 140),
            (true, ButtonTone::Secondary, true) => theme.border_highlight(),
            (true, ButtonTone::Secondary, false) => theme.border_subtle(),
        };
        draw_rounded_outline(canvas, self.rect, 7.0, 1.0, rim, with_alpha(rim, 70));

        let text_color = if !self.enabled {
            theme.ink_faint()
        } else if self.tone == ButtonTone::Danger {
            theme.danger_coral()
        } else {
            theme.ink_primary()
        };
        let tw = canvas.text_width(&self.label);
        let tx = (self.rect.x + (self.rect.w - tw) * 0.5).round();
        let ty = (self.rect.y + (self.rect.h - 9.0) * 0.5).round();
        canvas.draw_text(&self.label, tx, ty, text_color, true);
    }
}

/// Interactive single-line glass text field supporting masking, caret blink, and keyboard editing.
#[derive(Clone, Debug, Default)]
pub struct GlassTextField {
    pub value: String,
    pub max_len: usize,
    pub masked: bool,
    pub focused: bool,
}

impl GlassTextField {
    pub fn new(max_len: usize, masked: bool) -> Self {
        Self {
            value: String::new(),
            max_len,
            masked,
            focused: false,
        }
    }

    pub fn push_char(&mut self, c: char) {
        if !c.is_control() && self.value.chars().count() < self.max_len {
            self.value.push(c);
        }
    }

    pub fn backspace(&mut self) {
        self.value.pop();
    }

    pub fn clear(&mut self) {
        self.value.clear();
    }

    pub fn render_in(
        &self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        rect: Rect,
        placeholder: &str,
    ) {
        draw_rounded_rect(canvas, rect, 7.0, theme.input_fill(self.focused));
        let rim = if self.focused {
            theme.accent_soft()
        } else {
            theme.border_subtle()
        };
        draw_rounded_outline(canvas, rect, 7.0, 1.0, rim, with_alpha(rim, 90));

        let clip = canvas.begin_clip();
        let shown: String = if self.value.is_empty() {
            String::new()
        } else if self.masked {
            "*".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };

        let ty = (rect.y + (rect.h - 9.0) * 0.5).round();
        let tx = rect.x + 8.0;
        if shown.is_empty() && !self.focused {
            canvas.draw_text(placeholder, tx, ty, theme.ink_faint(), false);
        } else {
            // Keep right side visible when text exceeds field width
            let max_w = (rect.w - 18.0).max(16.0);
            let mut display = shown.as_str();
            while canvas.text_width(display) > max_w && display.len() > 1 {
                let next_idx = display.char_indices().nth(1).map_or(display.len(), |(i, _)| i);
                display = &display[next_idx..];
            }
            canvas.draw_text(display, tx, ty, theme.ink_primary(), false);
            if self.focused && ((canvas.time_secs() * 2.0) as u64).is_multiple_of(2) {
                let cx = tx + canvas.text_width(display) + 1.0;
                canvas.fill_rect(Rect::new(cx, rect.y + 4.0, 1.5, rect.h - 8.0), theme.accent_soft());
            }
        }
        canvas.end_clip(clip, rect.inset(2.0));
    }
}

/// Horizontal row hit-regions produced dynamically by rendering an account card.
#[derive(Clone, Debug)]
pub struct CardHitRects {
    pub account_id: String,
    pub row_rect: Rect,
    pub favorite_rect: Rect,
    pub refresh_rect: Option<Rect>,
    pub delete_rect: Rect,
}
