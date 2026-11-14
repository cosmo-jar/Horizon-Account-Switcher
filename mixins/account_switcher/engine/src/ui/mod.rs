// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

pub mod canvas;
pub mod controller;
pub mod theme;
pub mod widgets;

pub use canvas::{Rect, UiCanvas};
pub use controller::AccountSwitcherController;
pub use theme::{LiquidGlassTheme, argb, mix_argb, with_alpha};
