// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use std::sync::{Arc, OnceLock, RwLock};

#[derive(Clone, Default)]
struct OverrideState {
    /// `None` = fall back to launcher's `CURRENT`; `Some(inner)` = switched active session (`None` when offline).
    session: Option<Option<Arc<super::AuthSession>>>,
    player_name: Option<String>,
}

static STATE: OnceLock<RwLock<OverrideState>> = OnceLock::new();

fn store() -> &'static RwLock<OverrideState> {
    STATE.get_or_init(|| RwLock::new(OverrideState::default()))
}

/// Switches the active Minecraft session override to an authenticated Microsoft account.
pub fn set_online(name: &str, uuid: u128, xuid: Option<&str>, access_token: &str) {
    let mut session = super::AuthSession::new(name, uuid, access_token);
    session.xuid = xuid.map(str::to_owned);
    let session = Arc::new(session);
    if let Ok(mut guard) = store().write() {
        guard.session = Some(Some(Arc::clone(&session)));
        guard.player_name = Some(name.to_owned());
    }
    super::prepare_chat_keys_for(session, false);
}

/// Switches the active session override to offline mode with the given nickname.
pub fn set_offline(name: &str) {
    if let Ok(mut guard) = store().write() {
        guard.session = Some(None);
        guard.player_name = Some(name.to_owned());
    }
}

/// Clears any in-game session/name override, restoring the original launcher session.
pub fn clear_override() {
    if let Ok(mut guard) = store().write() {
        guard.session = None;
        guard.player_name = None;
    }
    if let Some(orig) = super::CURRENT.get() {
        super::prepare_chat_keys_for(Arc::clone(orig), false);
    }
}

/// Reads the original launcher `AuthSession` directly from `CURRENT` without override hooks.
pub fn raw_launcher_session() -> Option<Arc<super::AuthSession>> {
    super::CURRENT.get().cloned()
}

/// Returns `Some(session_opt)` when overridden in-game, or `None` to keep original launcher state.
pub fn override_session() -> Option<Option<Arc<super::AuthSession>>> {
    store().read().ok().and_then(|g| g.session.clone())
}

/// Returns overridden player name when switched in-game, or `None` to keep original launcher name.
pub fn override_name() -> Option<String> {
    store().read().ok().and_then(|g| g.player_name.clone())
}
