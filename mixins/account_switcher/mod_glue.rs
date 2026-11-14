// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use account_switcher_engine::{
    AccountSwitcherController, AccountSwitcherService, ClipboardBridge, FileAccountRepository,
    LauncherSessionInfo, Rect, SessionBridge, SwitcherError, UiCanvas,
};
use horizon_client_input::clipboard::{Clipboard, SystemClipboard};
use horizon_client_render::hud::{HudAtlas, HudBatch};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

pub const ID: &str = "account_switcher";
pub const NAME: &str = "Account Switcher";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str =
    "Liquid-glass in-game Microsoft & offline account switcher with encrypted vault storage.";

struct HorizonSessionBridge {
    initial_name: String,
    initial_launcher: Mutex<LauncherSessionInfo>,
}

impl HorizonSessionBridge {
    fn new(initial_name: String) -> Self {
        let initial_launcher =
            if let Some(raw) = horizon_client_render::auth::account_switcher_override::raw_launcher_session() {
                LauncherSessionInfo {
                    username: raw.name.clone(),
                    is_online: true,
                    uuid: Some(raw.uuid),
                }
            } else {
                LauncherSessionInfo {
                    username: initial_name.clone(),
                    is_online: false,
                    uuid: None,
                }
            };
        Self {
            initial_name,
            initial_launcher: Mutex::new(initial_launcher),
        }
    }
}

impl SessionBridge for HorizonSessionBridge {
    fn apply_online(
        &self,
        username: &str,
        uuid: u128,
        xuid: Option<&str>,
        access_token: &str,
    ) -> Result<(), SwitcherError> {
        horizon_client_render::auth::account_switcher_override::set_online(
            username,
            uuid,
            xuid,
            access_token,
        );
        Ok(())
    }

    fn apply_offline(&self, username: &str) -> Result<(), SwitcherError> {
        horizon_client_render::auth::account_switcher_override::set_offline(username);
        Ok(())
    }

    fn restore_launcher(&self) -> Result<(), SwitcherError> {
        horizon_client_render::auth::account_switcher_override::clear_override();
        Ok(())
    }

    fn current_identity(&self) -> (String, bool) {
        if let Some(session) = horizon_client_render::auth::current() {
            (session.name.clone(), true)
        } else if let Some(name) =
            horizon_client_render::auth::account_switcher_override::override_name()
        {
            (name, false)
        } else if let Some(info) = self.launcher_session() {
            (info.username, info.is_online)
        } else {
            (self.initial_name.clone(), false)
        }
    }

    fn launcher_session(&self) -> Option<LauncherSessionInfo> {
        if let Some(raw) =
            horizon_client_render::auth::account_switcher_override::raw_launcher_session()
        {
            let info = LauncherSessionInfo {
                username: raw.name.clone(),
                is_online: true,
                uuid: Some(raw.uuid),
            };
            if let Ok(mut guard) = self.initial_launcher.lock() {
                *guard = info.clone();
            }
            return Some(info);
        }
        self.initial_launcher.lock().ok().map(|g| g.clone())
    }
}

struct HorizonClipboardBridge {
    inner: Mutex<SystemClipboard>,
}

impl HorizonClipboardBridge {
    fn new() -> Self {
        Self {
            inner: Mutex::new(SystemClipboard::default()),
        }
    }
}

impl ClipboardBridge for HorizonClipboardBridge {
    fn get_text(&self) -> String {
        self.inner
            .lock()
            .map(|mut cb| cb.get())
            .unwrap_or_default()
    }

    fn set_text(&self, text: &str) {
        if let Ok(mut cb) = self.inner.lock() {
            cb.set(text);
        }
    }
}

struct HudBatchCanvas<'a> {
    batch: &'a mut HudBatch,
    atlas: &'a HudAtlas,
    elapsed: f32,
}

impl<'a> HudBatchCanvas<'a> {
    fn new(batch: &'a mut HudBatch, atlas: &'a HudAtlas, started: Instant) -> Self {
        Self {
            batch,
            atlas,
            elapsed: started.elapsed().as_secs_f32(),
        }
    }

    #[inline]
    const fn to_hud_rgba(argb: u32) -> u32 {
        let a = ((argb >> 24) & 0xFF) as u8;
        let r = ((argb >> 16) & 0xFF) as u8;
        let g = ((argb >> 8) & 0xFF) as u8;
        let b = (argb & 0xFF) as u8;
        horizon_client_render::hud::rgba(r, g, b, a)
    }
}

impl UiCanvas for HudBatchCanvas<'_> {
    fn screen_size(&self) -> (f32, f32) {
        (self.batch.width, self.batch.height)
    }

    fn time_secs(&self) -> f32 {
        self.elapsed
    }

    fn fill_rect(&mut self, rect: Rect, color: u32) {
        self.batch.fill(
            self.atlas,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Self::to_hud_rgba(color),
        );
    }

    fn fill_gradient(&mut self, rect: Rect, top: u32, bottom: u32) {
        self.batch.fill_gradient(
            self.atlas,
            (rect.x, rect.y, rect.w, rect.h),
            Self::to_hud_rgba(top),
            Self::to_hud_rgba(bottom),
        );
    }

    fn fill_triangle(&mut self, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], color: u32) {
        self.batch
            .fill_quad(self.atlas, [p0, p1, p2, p2], Self::to_hud_rgba(color));
    }

    fn draw_text(&mut self, text: &str, x: f32, y: f32, color: u32, shadow: bool) {
        self.batch
            .text(self.atlas, text, x, y, Self::to_hud_rgba(color), shadow);
    }

    fn text_width(&self, text: &str) -> f32 {
        HudBatch::text_width(self.atlas, text)
    }

    fn begin_clip(&mut self) -> usize {
        self.batch.vertices.len()
    }

    fn end_clip(&mut self, start_vertex: usize, rect: Rect) {
        self.batch.clip_since(
            start_vertex,
            rect.x,
            rect.y,
            rect.x + rect.w,
            rect.y + rect.h,
        );
    }
}

struct RuntimeState {
    controller: AccountSwitcherController,
    started: Instant,
}

static RUNTIME: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
static INITIAL_PLAYER_NAME: OnceLock<Option<String>> = OnceLock::new();

pub(super) fn sync_options(game: &mut crate::game::Game<'_>) {
    let base = INITIAL_PLAYER_NAME.get_or_init(|| game.options.player_name.clone());
    if !game.native_on(ID) {
        if horizon_client_render::auth::account_switcher_override::override_session().is_some()
            || horizon_client_render::auth::account_switcher_override::override_name().is_some()
        {
            horizon_client_render::auth::account_switcher_override::clear_override();
        }
        if game.options.player_name != *base {
            game.options.player_name = base.clone();
        }
        return;
    }
    let desired = horizon_client_render::auth::account_switcher_override::override_name()
        .map(Some)
        .unwrap_or_else(|| base.clone());
    if game.options.player_name != desired {
        game.options.player_name = desired;
    }
}

fn with_runtime<R>(
    data_dir: PathBuf,
    initial_name: String,
    f: impl FnOnce(&mut RuntimeState) -> R,
) -> Option<R> {
    let mtx = RUNTIME.get_or_init(|| {
        let repo = Box::new(FileAccountRepository::in_dir(&data_dir));
        let bridge = Arc::new(HorizonSessionBridge::new(initial_name));
        let svc = AccountSwitcherService::bootstrap(repo, bridge);
        let controller =
            AccountSwitcherController::new(svc).with_clipboard(Arc::new(HorizonClipboardBridge::new()));
        Mutex::new(RuntimeState {
            controller,
            started: Instant::now(),
        })
    });
    mtx.lock().ok().map(|mut guard| f(&mut guard))
}

pub(super) fn render_screen(
    game: &crate::game::Game<'_>,
    batch: &mut HudBatch,
    screen: &crate::mod_api::NativeScreenView<'_>,
) {
    if !game.native_on(ID) {
        return;
    }
    let data_dir = game.native_data_dir(ID);
    let player_name = game.options.player_name();
    let lang_code = horizon_client_core::lang::code();
    let mouse = screen.mouse;
    let screen_id = screen.id;
    let atlas = &game.gui.atlas;

    let _ = with_runtime(data_dir, player_name, |rt| {
        let mut canvas = HudBatchCanvas::new(batch, atlas, rt.started);
        rt.controller.render(&mut canvas, screen_id, mouse, &lang_code);
    });
}

pub(super) fn mouse_press(
    game: &crate::game::Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    let consumed = with_runtime(game.native_data_dir(ID), game.options.player_name(), |rt| {
        rt.controller.handle_mouse_press(screen.mouse, button)
    })
    .unwrap_or(false);
    if consumed {
        *canceled = true;
    }
}

pub(super) fn mouse_scroll(game: &crate::game::Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    if !game.native_on(ID) {
        return;
    }
    let consumed = with_runtime(game.native_data_dir(ID), game.options.player_name(), |rt| {
        rt.controller.handle_mouse_scroll(scroll[1])
    })
    .unwrap_or(false);
    if consumed {
        *canceled = true;
    }
}

pub(super) fn key_press(
    game: &crate::game::Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    key: &str,
    modifiers: u32,
    canceled: &mut bool,
) {
    if !game.native_on(ID) {
        return;
    }
    let consumed = with_runtime(game.native_data_dir(ID), game.options.player_name(), |rt| {
        rt.controller.handle_key_press(key, modifiers, screen.id)
    })
    .unwrap_or(false);
    if consumed {
        *canceled = true;
    }
}

pub(super) fn char_typed(game: &crate::game::Game<'_>, c: char, canceled: &mut bool) {
    if !game.native_on(ID) {
        return;
    }
    let consumed = with_runtime(game.native_data_dir(ID), game.options.player_name(), |rt| {
        rt.controller.handle_char_typed(c)
    })
    .unwrap_or(false);
    if consumed {
        *canceled = true;
    }
}

pub(super) fn settings_buttons(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != ID {
        return;
    }
    let is_open = with_runtime(game.native_data_dir(ID), game.options.player_name(), |rt| {
        rt.controller.is_open()
    })
    .unwrap_or(false);
    buttons.push(if is_open {
        "Close Account Switcher".into()
    } else {
        "Open Account Switcher (Alt+A)".into()
    });
}

pub(super) fn settings_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    if mod_id != ID || button != 0 {
        return;
    }
    let _ = with_runtime(game.native_data_dir(ID), game.options.player_name(), |rt| {
        if rt.controller.is_open() {
            rt.controller.close_overlay();
        } else {
            rt.controller.open_overlay();
        }
    });
}
