// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

pub mod i18n;
pub mod service;

pub use i18n::{LanguagePack, Locale};
pub use service::{
    AccountFilter, AccountSwitcherService, ClipboardBridge, LauncherSessionInfo, NoopClipboard,
    SessionBridge,
};
