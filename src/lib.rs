// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use mod_api::{
    self, ClientHost, ClientTickPreEvent, Flow, Hooks, ScreenCharTypedPreEvent,
    ScreenKeyPressedPreEvent, ScreenMouseButtonPressedPreEvent, ScreenMouseScrolledPreEvent,
    ScreenRenderPostEvent,
};

pub mod auth;
pub mod crypto;
pub mod domain;
pub mod session;
pub mod storage;
pub mod ui;

pub use auth::{HttpBridge, HttpResponse, OAuthLoginKind, offline_uuid_for};
pub use domain::{AccountIdentity, AccountKind, AccountRecord, SwitcherError};
pub use session::{AccountSwitcherService, ClipboardBridge, LauncherSessionInfo, SessionBridge};
pub use storage::FileAccountRepository;
pub use ui::{AccountSwitcherController, Rect, UiCanvas};

#[derive(Default)]
struct Mod;

// Subscribing to these hooks ensures Horizon always dispatches the underlying
// `native_client_tick_pre` and `native_screen_*` entry points attached in `horizon-mod.toml`.
#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn client_tick_pre(&self, _: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {}

    fn screen_render_post(&self, _: &dyn ClientHost, _: &mut ScreenRenderPostEvent<'_>) {}

    fn screen_mouse_button_pressed_pre(
        &self,
        _: &dyn ClientHost,
        _: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        Flow::Continue
    }

    fn screen_mouse_scrolled_pre(
        &self,
        _: &dyn ClientHost,
        _: &mut ScreenMouseScrolledPreEvent<'_>,
    ) -> Flow {
        Flow::Continue
    }

    fn screen_key_pressed_pre(
        &self,
        _: &dyn ClientHost,
        _: &mut ScreenKeyPressedPreEvent<'_>,
    ) -> Flow {
        Flow::Continue
    }

    fn screen_char_typed_pre(
        &self,
        _: &dyn ClientHost,
        _: &mut ScreenCharTypedPreEvent<'_>,
    ) -> Flow {
        Flow::Continue
    }
}
