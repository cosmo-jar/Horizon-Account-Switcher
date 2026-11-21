// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use horizon_client::auth::AuthSession;
use horizon_client::clipboard::{Clipboard, SystemClipboard};
use horizon_client::hud::{HudAtlas, HudBatch, rgba};
use mod_account_switcher::{
    AccountSwitcherController, AccountSwitcherService, ClipboardBridge, FileAccountRepository,
    HttpBridge, HttpResponse, LauncherSessionInfo, Rect, SessionBridge, SwitcherError, UiCanvas,
};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::{Duration, Instant},
};

pub const ID: &str = "account_switcher";

#[derive(Clone, Default)]
struct OverrideState {
    /// `None` = fall back to launcher's `current()`; `Some(inner)` = switched active session (`None` when offline).
    session: Option<Option<Arc<AuthSession>>>,
    player_name: Option<String>,
}

static OVERRIDE_STATE: OnceLock<RwLock<OverrideState>> = OnceLock::new();

fn override_store() -> &'static RwLock<OverrideState> {
    OVERRIDE_STATE.get_or_init(|| RwLock::new(OverrideState::default()))
}

fn set_online(name: &str, uuid: u128, xuid: Option<&str>, access_token: &str) {
    let mut session = AuthSession::new(name, uuid, access_token);
    session.xuid = xuid.map(str::to_owned);
    let session = Arc::new(session);
    if let Ok(mut guard) = override_store().write() {
        guard.session = Some(Some(Arc::clone(&session)));
        guard.player_name = Some(name.to_owned());
    }
    horizon_client::auth::prepare_chat_keys_for(session, false);
}

fn set_offline(name: &str) {
    if let Ok(mut guard) = override_store().write() {
        guard.session = Some(None);
        guard.player_name = Some(name.to_owned());
    }
}

fn clear_override() {
    if let Ok(mut guard) = override_store().write() {
        guard.session = None;
        guard.player_name = None;
    }
    if let Some(orig) = horizon_client::auth::current() {
        horizon_client::auth::prepare_chat_keys_for(orig, false);
    }
}

fn override_session() -> Option<Option<Arc<AuthSession>>> {
    override_store().read().ok().and_then(|g| g.session.clone())
}

fn override_name() -> Option<String> {
    override_store()
        .read()
        .ok()
        .and_then(|g| g.player_name.clone())
}

fn is_mod_enabled(game: &super::Game<'_>) -> bool {
    !game.settings.mods.disabled.contains(ID)
}

pub(in crate::game) fn effective_join_name(game: &super::Game<'_>, original: &str) -> String {
    if !is_mod_enabled(game) {
        return original.to_owned();
    }
    override_name().unwrap_or_else(|| original.to_owned())
}

pub(in crate::game) fn effective_remote_auth(
    game: &super::Game<'_>,
    original: Option<Arc<AuthSession>>,
) -> Option<Arc<AuthSession>> {
    if game.options.split_guest || !is_mod_enabled(game) {
        return original;
    }
    override_session().unwrap_or(original)
}

struct HorizonSessionBridge {
    initial_name: String,
    initial_launcher: Mutex<LauncherSessionInfo>,
}

impl HorizonSessionBridge {
    fn new(initial_name: String) -> Self {
        let initial_launcher = if let Some(raw) = horizon_client::auth::current() {
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
        set_online(username, uuid, xuid, access_token);
        Ok(())
    }

    fn apply_offline(&self, username: &str) -> Result<(), SwitcherError> {
        set_offline(username);
        Ok(())
    }

    fn restore_launcher(&self) -> Result<(), SwitcherError> {
        clear_override();
        Ok(())
    }

    fn current_identity(&self) -> (String, bool) {
        if let Some(overridden) = override_session() {
            match overridden {
                Some(session) => return (session.name.clone(), true),
                None => {
                    let name = override_name().unwrap_or_else(|| self.initial_name.clone());
                    return (name, false);
                }
            }
        }
        if let Some(info) = self.launcher_session() {
            (info.username, info.is_online)
        } else {
            (self.initial_name.clone(), false)
        }
    }

    fn launcher_session(&self) -> Option<LauncherSessionInfo> {
        if let Some(raw) = horizon_client::auth::current() {
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

struct HorizonHttpBridge {
    agent: ureq::Agent,
}

impl HorizonHttpBridge {
    fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(12))
            .timeout_read(Duration::from_secs(20))
            .build();
        Self { agent }
    }
}

impl HttpBridge for HorizonHttpBridge {
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<HttpResponse, SwitcherError> {
        match self.agent.post(url).send_form(form) {
            Ok(ok) => {
                let status = ok.status();
                let body = ok
                    .into_string()
                    .map_err(|e| SwitcherError::Network(format!("Read HTTP response: {e}")))?;
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::Status(status, err_resp)) => Ok(HttpResponse {
                status,
                body: err_resp.into_string().unwrap_or_default(),
            }),
            Err(e) => Err(SwitcherError::Network(format!("HTTP POST failed: {e}"))),
        }
    }

    fn post_json(&self, url: &str, json_body: &str) -> Result<HttpResponse, SwitcherError> {
        match self
            .agent
            .post(url)
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_string(json_body)
        {
            Ok(ok) => {
                let status = ok.status();
                let body = ok
                    .into_string()
                    .map_err(|e| SwitcherError::Network(format!("Read JSON response: {e}")))?;
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::Status(status, err_resp)) => Ok(HttpResponse {
                status,
                body: err_resp.into_string().unwrap_or_default(),
            }),
            Err(e) => Err(SwitcherError::OAuth(format!("HTTP JSON POST failed: {e}"))),
        }
    }

    fn get_text(&self, url: &str, bearer: Option<&str>) -> Result<String, SwitcherError> {
        let mut req = self.agent.get(url);
        if let Some(tok) = bearer {
            req = req.set("Authorization", &format!("Bearer {tok}"));
        }
        req.call()
            .map_err(|e| SwitcherError::Network(format!("HTTP GET failed: {e}")))?
            .into_string()
            .map_err(|e| SwitcherError::Network(format!("Read GET response: {e}")))
    }

    fn get_bytes(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, SwitcherError> {
        let resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| SwitcherError::Network(format!("HTTP GET bytes failed: {e}")))?;
        let mut out = Vec::new();
        resp.into_reader()
            .take(max_bytes as u64)
            .read_to_end(&mut out)
            .map_err(|e| SwitcherError::Network(format!("Read GET bytes: {e}")))?;
        Ok(out)
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
        rgba(r, g, b, a)
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

pub(in crate::game) fn sync_options(game: &mut super::Game<'_>) {
    let base = INITIAL_PLAYER_NAME.get_or_init(|| game.options.player_name.clone());
    if !is_mod_enabled(game) {
        if override_session().is_some() || override_name().is_some() {
            clear_override();
        }
        if game.options.player_name != *base {
            game.options.player_name = base.clone();
        }
        return;
    }
    let desired = override_name().map(Some).unwrap_or_else(|| base.clone());
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
        let svc = AccountSwitcherService::bootstrap_with_http(
            repo,
            bridge,
            Arc::new(HorizonHttpBridge::new()),
        );
        let controller =
            AccountSwitcherController::new(svc).with_clipboard(Arc::new(HorizonClipboardBridge::new()));
        Mutex::new(RuntimeState {
            controller,
            started: Instant::now(),
        })
    });
    mtx.lock().ok().map(|mut guard| f(&mut guard))
}

pub(in crate::game) fn render_screen(
    game: &super::Game<'_>,
    batch: &mut HudBatch,
    screen: &mod_api::ScreenView<'_>,
) {
    if !is_mod_enabled(game) {
        return;
    }
    let data_dir = game.native_data_dir(ID);
    let player_name = game.options.player_name();
    let lang_code = horizon_client::lang::code();
    let mouse = screen.mouse;
    let screen_id = screen.id;
    let atlas = &game.gui.atlas;

    let _ = with_runtime(data_dir, player_name, |rt| {
        let mut canvas = HudBatchCanvas::new(batch, atlas, rt.started);
        rt.controller.render(&mut canvas, screen_id, mouse, &lang_code);
    });
}

pub(in crate::game) fn mouse_press(
    game: &super::Game<'_>,
    screen: &mod_api::ScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    if !is_mod_enabled(game) {
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

pub(in crate::game) fn mouse_scroll(
    game: &super::Game<'_>,
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    if !is_mod_enabled(game) {
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

pub(in crate::game) fn key_press(
    game: &super::Game<'_>,
    screen: &mod_api::ScreenView<'_>,
    key: &str,
    modifiers: u32,
    canceled: &mut bool,
) {
    if !is_mod_enabled(game) {
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

pub(in crate::game) fn char_typed(game: &super::Game<'_>, c: char, canceled: &mut bool) {
    if !is_mod_enabled(game) {
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
