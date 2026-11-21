// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

/// Packs 8-bit color channels into Horizon `HudBatch` `0xAARRGGBB` format.
pub const fn argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Replaces the alpha byte of an existing `0xAARRGGBB` color while keeping RGB intact.
pub const fn with_alpha(rgb_or_argb: u32, alpha: u8) -> u32 {
    (rgb_or_argb & 0x00FF_FFFF) | ((alpha as u32) << 24)
}

/// Linearly interpolates two `0xAARRGGBB` colors at fraction `t` in `[0.0, 1.0]`.
pub fn mix_argb(c0: u32, c1: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |shift: u32| -> u32 {
        let a = ((c0 >> shift) & 0xFF) as f32;
        let b = ((c1 >> shift) & 0xFF) as f32;
        ((a + (b - a) * t).round().clamp(0.0, 255.0)) as u32
    };
    (ch(24) << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Liquid-glass palette harmonized with Horizon's `horizon-theme::glass` tokens.
#[derive(Clone, Copy, Debug)]
pub struct LiquidGlassTheme {
    pub accent_rgb: u32,
    pub opacity: f32,
}

impl LiquidGlassTheme {
    pub fn new(accent_rgb: u32, opacity: f32) -> Self {
        Self {
            accent_rgb: accent_rgb & 0x00FF_FFFF,
            opacity: opacity.clamp(0.45, 0.98),
        }
    }

    pub fn scrim(&self) -> u32 {
        argb(7, 5, 15, (148.0 * self.opacity) as u8)
    }

    pub fn panel_top(&self) -> u32 {
        let base = mix_argb(with_alpha(self.accent_rgb, 255), argb(26, 24, 52, 255), 0.84);
        with_alpha(base, (218.0 * self.opacity) as u8)
    }

    pub fn panel_bottom(&self) -> u32 {
        with_alpha(0x0B0A1A, (236.0 * self.opacity) as u8)
    }

    pub fn specular_top_glint(&self) -> u32 {
        argb(255, 255, 255, 34)
    }

    pub fn border_highlight(&self) -> u32 {
        argb(255, 255, 255, 86)
    }

    pub fn border_subtle(&self) -> u32 {
        mix_argb(with_alpha(self.accent_rgb, 110), argb(255, 255, 255, 36), 0.45)
    }

    pub fn card_fill(&self, hovered: bool, active: bool) -> u32 {
        if active {
            let tint = mix_argb(with_alpha(self.accent_rgb, 255), argb(28, 26, 56, 255), 0.72);
            with_alpha(tint, if hovered { 215 } else { 195 })
        } else if hovered {
            argb(255, 255, 255, 36)
        } else {
            argb(255, 255, 255, 19)
        }
    }

    pub fn button_fill(&self, primary: bool, hovered: bool, danger: bool) -> u32 {
        if danger {
            if hovered {
                argb(255, 107, 107, 78)
            } else {
                argb(255, 107, 107, 42)
            }
        } else if primary {
            let bright = mix_argb(with_alpha(self.accent_rgb, 255), 0xFFFFFFFF, 0.16);
            if hovered {
                with_alpha(bright, 245)
            } else {
                with_alpha(self.accent_rgb, 224)
            }
        } else if hovered {
            argb(255, 255, 255, 42)
        } else {
            argb(255, 255, 255, 22)
        }
    }

    pub fn input_fill(&self, focused: bool) -> u32 {
        if focused {
            argb(18, 16, 36, 220)
        } else {
            argb(12, 10, 24, 180)
        }
    }

    pub fn ink_primary(&self) -> u32 {
        0xFFFFFFFF
    }

    pub fn ink_muted(&self) -> u32 {
        argb(226, 224, 246, 178)
    }

    pub fn ink_faint(&self) -> u32 {
        argb(200, 198, 225, 128)
    }

    pub fn live_emerald(&self) -> u32 {
        0xFF4ADE80
    }

    pub fn warn_amber(&self) -> u32 {
        0xFFF2C46B
    }

    pub fn danger_coral(&self) -> u32 {
        0xFFFF8A8A
    }

    pub fn accent_soft(&self) -> u32 {
        mix_argb(with_alpha(self.accent_rgb, 255), 0xFFFFFFFF, 0.42)
    }
}
