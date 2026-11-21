// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use std::f32::consts::FRAC_PI_2;

use super::theme::{LiquidGlassTheme, argb, mix_argb, with_alpha};

/// Axis-aligned rectangle in GUI logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Returns `true` when pointer `(px, py)` lies inside `[x, x + w) x [y, y + h)`.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// Shrinks rectangle symmetrically on all sides.
    pub fn inset(&self, pad: f32) -> Self {
        Self::new(
            self.x + pad,
            self.y + pad,
            (self.w - pad * 2.0).max(0.0),
            (self.h - pad * 2.0).max(0.0),
        )
    }
}

/// Abstract vector/text rendering surface implemented by `HudBatch` in-game and mocks in tests.
pub trait UiCanvas {
    fn screen_size(&self) -> (f32, f32);
    fn time_secs(&self) -> f32;
    fn fill_rect(&mut self, rect: Rect, color: u32);
    fn fill_gradient(&mut self, rect: Rect, top: u32, bottom: u32);
    fn fill_triangle(&mut self, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], color: u32);
    fn draw_text(&mut self, text: &str, x: f32, y: f32, color: u32, shadow: bool);
    fn text_width(&self, text: &str) -> f32;
    fn begin_clip(&mut self) -> usize;
    fn end_clip(&mut self, start_vertex: usize, rect: Rect);
}

/// Draws a filled rounded rectangle using rectangle core strips and triangulated corner fans.
pub fn draw_rounded_rect(canvas: &mut dyn UiCanvas, rect: Rect, radius: f32, color: u32) {
    let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
    if r <= 0.5 {
        canvas.fill_rect(rect, color);
        return;
    }

    // Central body + top/bottom horizontal strips between corner centers
    canvas.fill_rect(
        Rect::new(rect.x + r, rect.y, rect.w - r * 2.0, rect.h),
        color,
    );
    canvas.fill_rect(
        Rect::new(rect.x, rect.y + r, r, rect.h - r * 2.0),
        color,
    );
    canvas.fill_rect(
        Rect::new(rect.x + rect.w - r, rect.y + r, r, rect.h - r * 2.0),
        color,
    );

    let segs = 6usize;
    let corners = [
        ([rect.x + r, rect.y + r], std::f32::consts::PI, std::f32::consts::PI + FRAC_PI_2),
        ([rect.x + rect.w - r, rect.y + r], -FRAC_PI_2, 0.0),
        ([rect.x + rect.w - r, rect.y + rect.h - r], 0.0, FRAC_PI_2),
        ([rect.x + r, rect.y + rect.h - r], FRAC_PI_2, std::f32::consts::PI),
    ];

    for (center, a0, a1) in corners {
        for s in 0..segs {
            let t0 = a0 + (a1 - a0) * (s as f32 / segs as f32);
            let t1 = a0 + (a1 - a0) * ((s + 1) as f32 / segs as f32);
            let p1 = [center[0] + r * t0.cos(), center[1] + r * t0.sin()];
            let p2 = [center[0] + r * t1.cos(), center[1] + r * t1.sin()];
            canvas.fill_triangle(center, p1, p2, color);
        }
    }
}

/// Draws a crisp 1px rounded outline stroke around `rect`.
pub fn draw_rounded_outline(
    canvas: &mut dyn UiCanvas,
    rect: Rect,
    radius: f32,
    thickness: f32,
    color_top: u32,
    color_bottom: u32,
) {
    let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
    let t = thickness.max(1.0);

    // Top & bottom straight rim lines
    canvas.fill_rect(Rect::new(rect.x + r, rect.y, rect.w - r * 2.0, t), color_top);
    canvas.fill_rect(
        Rect::new(rect.x + r, rect.y + rect.h - t, rect.w - r * 2.0, t),
        color_bottom,
    );
    // Left & right straight rim lines
    canvas.fill_gradient(
        Rect::new(rect.x, rect.y + r, t, rect.h - r * 2.0),
        color_top,
        color_bottom,
    );
    canvas.fill_gradient(
        Rect::new(rect.x + rect.w - t, rect.y + r, t, rect.h - r * 2.0),
        color_top,
        color_bottom,
    );

    if r <= 0.5 {
        return;
    }
    let inner_r = (r - t).max(0.0);
    let segs = 6usize;
    let corners = [
        ([rect.x + r, rect.y + r], std::f32::consts::PI, std::f32::consts::PI + FRAC_PI_2, color_top),
        ([rect.x + rect.w - r, rect.y + r], -FRAC_PI_2, 0.0, color_top),
        ([rect.x + rect.w - r, rect.y + rect.h - r], 0.0, FRAC_PI_2, color_bottom),
        ([rect.x + r, rect.y + rect.h - r], FRAC_PI_2, std::f32::consts::PI, color_bottom),
    ];

    for (center, a0, a1, col) in corners {
        for s in 0..segs {
            let t0 = a0 + (a1 - a0) * (s as f32 / segs as f32);
            let t1 = a0 + (a1 - a0) * ((s + 1) as f32 / segs as f32);
            let o0 = [center[0] + r * t0.cos(), center[1] + r * t0.sin()];
            let o1 = [center[0] + r * t1.cos(), center[1] + r * t1.sin()];
            let i0 = [center[0] + inner_r * t0.cos(), center[1] + inner_r * t0.sin()];
            let i1 = [center[0] + inner_r * t1.cos(), center[1] + inner_r * t1.sin()];
            canvas.fill_triangle(o0, o1, i1, col);
            canvas.fill_triangle(o0, i1, i0, col);
        }
    }
}

/// Draws a vertical gradient strictly clipped inside a rounded-rectangle silhouette.
pub fn fill_rounded_gradient(
    canvas: &mut dyn UiCanvas,
    rect: Rect,
    radius: f32,
    top_color: u32,
    bottom_color: u32,
) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
    if r <= 0.5 {
        canvas.fill_gradient(rect, top_color, bottom_color);
        return;
    }

    let cap = r.ceil() as usize;
    let total_h = rect.h;
    // Top curved corner band
    for step in 0..cap {
        let y0 = step as f32;
        if y0 >= r || y0 >= total_h {
            break;
        }
        let y1 = (y0 + 1.0).min(r).min(total_h);
        let mid_y = (y0 + y1) * 0.5;
        let dy = (r - mid_y).clamp(0.0, r);
        let dx = (r - (r * r - dy * dy).max(0.0).sqrt()).max(0.0);
        let t = (mid_y / total_h).clamp(0.0, 1.0);
        canvas.fill_rect(
            Rect::new(rect.x + dx, rect.y + y0, (rect.w - dx * 2.0).max(0.0), y1 - y0),
            mix_argb(top_color, bottom_color, t),
        );
    }

    // Middle straight band
    let mid_h = (total_h - r * 2.0).max(0.0);
    if mid_h > 0.0 {
        let t0 = (r / total_h).clamp(0.0, 1.0);
        let t1 = ((total_h - r) / total_h).clamp(0.0, 1.0);
        canvas.fill_gradient(
            Rect::new(rect.x, rect.y + r, rect.w, mid_h),
            mix_argb(top_color, bottom_color, t0),
            mix_argb(top_color, bottom_color, t1),
        );
    }

    // Bottom curved corner band
    let bot_start = (total_h - r).max(r);
    for step in 0..cap {
        let local_y0 = bot_start + step as f32;
        if local_y0 >= total_h {
            break;
        }
        let local_y1 = (local_y0 + 1.0).min(total_h);
        let mid_y = (local_y0 + local_y1) * 0.5;
        let dy = (mid_y - (total_h - r)).clamp(0.0, r);
        let dx = (r - (r * r - dy * dy).max(0.0).sqrt()).max(0.0);
        let t = (mid_y / total_h).clamp(0.0, 1.0);
        canvas.fill_rect(
            Rect::new(
                rect.x + dx,
                rect.y + local_y0,
                (rect.w - dx * 2.0).max(0.0),
                local_y1 - local_y0,
            ),
            mix_argb(top_color, bottom_color, t),
        );
    }
}

/// Renders text constrained to `max_w` pixels by truncating cleanly with `...` and GPU clipping.
pub fn draw_truncated_text(
    canvas: &mut dyn UiCanvas,
    text: &str,
    rect: Rect,
    color: u32,
    shadow: bool,
) {
    if rect.w <= 8.0 || text.is_empty() {
        return;
    }
    let clip = canvas.begin_clip();
    if canvas.text_width(text) <= rect.w {
        canvas.draw_text(text, rect.x, rect.y, color, shadow);
    } else {
        let ellipsis = "...";
        let ell_w = canvas.text_width(ellipsis);
        let mut out = String::new();
        for ch in text.chars() {
            out.push(ch);
            if canvas.text_width(&out) + ell_w > rect.w {
                out.pop();
                break;
            }
        }
        out.push_str(ellipsis);
        canvas.draw_text(&out, rect.x, rect.y, color, shadow);
    }
    canvas.end_clip(clip, rect);
}

/// Renders multi-line wrapped text inside `bounds`, clipping to `bounds` and capping line count.
pub fn draw_wrapped_text(
    canvas: &mut dyn UiCanvas,
    text: &str,
    bounds: Rect,
    line_height: f32,
    color: u32,
    shadow: bool,
) {
    if bounds.w <= 12.0 || bounds.h < line_height || text.is_empty() {
        return;
    }
    let max_lines = (bounds.h / line_height).floor().max(1.0) as usize;
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else {
            let candidate = format!("{current} {word}");
            if canvas.text_width(&candidate) <= bounds.w {
                current = candidate;
            } else {
                lines.push(current);
                current = word.to_owned();
                if lines.len() >= max_lines {
                    break;
                }
            }
        }
    }
    if lines.len() < max_lines && !current.is_empty() {
        lines.push(current);
    }

    let clip = canvas.begin_clip();
    for (idx, line) in lines.into_iter().take(max_lines).enumerate() {
        let y = bounds.y + idx as f32 * line_height;
        canvas.draw_text(&line, bounds.x, y, color, shadow);
    }
    canvas.end_clip(clip, bounds);
}

/// Renders a multi-layered liquid glass surface with ambient shadow, frosted gradient, top sheen, and refractive rim.
pub fn draw_liquid_glass_panel(
    canvas: &mut dyn UiCanvas,
    theme: &LiquidGlassTheme,
    rect: Rect,
    radius: f32,
) {
    // 1. Soft layered drop shadow for depth above Minecraft/Horizon panorama
    for (expand, alpha) in [(6.0, 22), (3.0, 38), (1.0, 62)] {
        draw_rounded_rect(
            canvas,
            Rect::new(
                rect.x - expand * 0.5,
                rect.y + expand * 0.65,
                rect.w + expand,
                rect.h + expand,
            ),
            radius + expand * 0.4,
            argb(5, 4, 14, alpha),
        );
    }

    // 2. Frosted base gradient clipped cleanly inside rounded silhouette
    fill_rounded_gradient(canvas, rect, radius, theme.panel_top(), theme.panel_bottom());

    // 3. Upper liquid refraction sheen strip fitted inside curved top contour
    let inner = rect.inset(1.0);
    let inner_r = (radius - 1.0).max(0.0);
    let sheen_h = (inner.h * 0.28).clamp(16.0, 64.0).min(inner.h);
    fill_rounded_gradient(
        canvas,
        Rect::new(inner.x, inner.y, inner.w, sheen_h),
        inner_r,
        theme.specular_top_glint(),
        argb(255, 255, 255, 0),
    );

    // 4. Specular glass rim highlight (top/left crisp white glint, bottom-right violet mist)
    draw_rounded_outline(
        canvas,
        rect,
        radius,
        1.0,
        theme.border_highlight(),
        theme.border_subtle(),
    );
}

/// Renders an `8x8` pixel-art avatar inside a sleek rounded frosted frame.
pub fn draw_avatar_grid(
    canvas: &mut dyn UiCanvas,
    grid: &[u32; 64],
    rect: Rect,
    border_color: u32,
) {
    draw_rounded_rect(
        canvas,
        Rect::new(rect.x - 1.5, rect.y - 1.5, rect.w + 3.0, rect.h + 3.0),
        4.0,
        with_alpha(border_color, 210),
    );
    let cell_w = rect.w / 8.0;
    let cell_h = rect.h / 8.0;
    for row in 0..8 {
        for col in 0..8 {
            let color = grid[row * 8 + col];
            canvas.fill_rect(
                Rect::new(
                    rect.x + col as f32 * cell_w,
                    rect.y + row as f32 * cell_h,
                    cell_w + 0.05,
                    cell_h + 0.05,
                ),
                color,
            );
        }
    }
    // Gloss diagonal highlight corner over skin head
    canvas.fill_gradient(
        Rect::new(rect.x, rect.y, rect.w, rect.h * 0.36),
        argb(255, 255, 255, 38),
        argb(255, 255, 255, 0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingCanvas {
        rects: Vec<Rect>,
    }

    impl UiCanvas for RecordingCanvas {
        fn screen_size(&self) -> (f32, f32) {
            (854.0, 480.0)
        }
        fn time_secs(&self) -> f32 {
            0.0
        }
        fn fill_rect(&mut self, rect: Rect, _color: u32) {
            self.rects.push(rect);
        }
        fn fill_gradient(&mut self, rect: Rect, _top: u32, _bottom: u32) {
            self.rects.push(rect);
        }
        fn fill_triangle(&mut self, _p0: [f32; 2], _p1: [f32; 2], _p2: [f32; 2], _color: u32) {}
        fn draw_text(&mut self, _text: &str, _x: f32, _y: f32, _color: u32, _shadow: bool) {}
        fn text_width(&self, text: &str) -> f32 {
            text.len() as f32 * 6.0
        }
        fn begin_clip(&mut self) -> usize {
            0
        }
        fn end_clip(&mut self, _start_vertex: usize, _rect: Rect) {}
    }

    #[test]
    fn rounded_gradient_insets_top_and_bottom_rows_inside_corner_arcs() {
        let mut canvas = RecordingCanvas { rects: Vec::new() };
        let panel = Rect::new(100.0, 50.0, 400.0, 200.0);
        let radius = 16.0;
        fill_rounded_gradient(&mut canvas, panel, radius, argb(10, 20, 40, 220), argb(10, 12, 24, 240));

        let top_row = canvas.rects.first().copied().unwrap();
        assert!(top_row.x > panel.x + 8.0);
        assert!(top_row.x + top_row.w < panel.x + panel.w - 8.0);

        let bottom_row = canvas.rects.last().copied().unwrap();
        assert!(bottom_row.x > panel.x + 8.0);
        assert!(bottom_row.x + bottom_row.w < panel.x + panel.w - 8.0);
    }
}
