// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

pub mod auth;
pub mod crypto;
pub mod domain;
pub mod session;
pub mod storage;
pub mod ui;

pub use auth::{OAuthLoginKind, offline_uuid_for};
pub use domain::{AccountIdentity, AccountKind, AccountRecord, SwitcherError};
pub use session::{AccountSwitcherService, ClipboardBridge, LauncherSessionInfo, SessionBridge};
pub use storage::FileAccountRepository;
pub use ui::{AccountSwitcherController, Rect, UiCanvas};
