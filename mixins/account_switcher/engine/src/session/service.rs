// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
};

use crate::{
    auth::{
        DeviceCodePrompt, MicrosoftOAuthProvider, OAuthLoginKind, OAuthProgressEvent,
        OfflineAuthProvider, SkinAvatarCache,
    },
    crypto::{EncryptionMode, cipher::random_bytes},
    domain::{
        AccountIdentity, AccountKind, AccountRecord, AuthStatusBadge, SwitcherError,
        account::unix_now,
    },
    storage::{AccountRepository, LoadedVault, ModSettings, VaultState},
};

/// Filter tabs available in the main account list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountFilter {
    All,
    Microsoft,
    Offline,
}

/// Active state of an asynchronous Microsoft login or token refresh job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthJobState {
    Idle,
    Running {
        stage: String,
        device_code: Option<DeviceCodePrompt>,
        browser_url: Option<String>,
    },
    Succeeded(String),
    Failed(String),
}

enum BackgroundJobMsg {
    OAuth(OAuthProgressEvent),
    RefreshComplete {
        account_id: String,
        switch_after: bool,
        result: Result<crate::auth::MicrosoftAuthResult, String>,
    },
}

/// System clipboard abstraction implemented by `mod_glue.rs` using Horizon's clipboard.
pub trait ClipboardBridge: Send + Sync {
    fn get_text(&self) -> String;
    fn set_text(&self, text: &str);
}

#[derive(Default)]
pub struct NoopClipboard;

impl ClipboardBridge for NoopClipboard {
    fn get_text(&self) -> String {
        String::new()
    }

    fn set_text(&self, _text: &str) {}
}

/// Runtime snapshot of the session Horizon Launcher originally started with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherSessionInfo {
    pub username: String,
    pub is_online: bool,
    pub uuid: Option<u128>,
}

/// Runtime hook abstraction implemented by `mod_glue.rs` to mutate Horizon's active identity.
pub trait SessionBridge: Send + Sync {
    fn apply_online(
        &self,
        username: &str,
        uuid: u128,
        xuid: Option<&str>,
        access_token: &str,
    ) -> Result<(), SwitcherError>;
    fn apply_offline(&self, username: &str) -> Result<(), SwitcherError>;
    fn restore_launcher(&self) -> Result<(), SwitcherError>;
    fn current_identity(&self) -> (String, bool);
    fn launcher_session(&self) -> Option<LauncherSessionInfo>;
}

/// High-level application service coordinating vault I/O, OAuth jobs, skins, and session swaps.
pub struct AccountSwitcherService {
    repo: Box<dyn AccountRepository + Send + Sync>,
    bridge: Arc<dyn SessionBridge>,
    state: VaultState,
    encryption_mode: EncryptionMode,
    unlocked: bool,
    cached_passphrase: Option<String>,
    skins: SkinAvatarCache,
    job_rx: Option<Receiver<BackgroundJobMsg>>,
    job_cancel: Option<Arc<AtomicBool>>,
    pub auth_job: AuthJobState,
    pub banner_message: Option<(String, bool)>,
}

impl AccountSwitcherService {
    /// Initializes the service, loading and decrypting the vault if possible.
    pub fn bootstrap(
        repo: Box<dyn AccountRepository + Send + Sync>,
        bridge: Arc<dyn SessionBridge>,
    ) -> Self {
        let mut skins = SkinAvatarCache::new();
        let (state, encryption_mode, unlocked, banner) = match repo.load(None) {
            Ok(LoadedVault::Unlocked { state, mode }) => {
                for acc in &state.accounts {
                    if let Some(grid) = acc.profile().head_grid() {
                        skins.seed(acc.id(), grid);
                    } else if acc.is_online() {
                        skins.ensure_requested(
                            acc.id(),
                            &format!("{:032x}", acc.uuid()),
                            acc.profile().skin_url.clone(),
                        );
                    }
                }
                (state, mode, true, None)
            }
            Ok(LoadedVault::Locked { settings, mode }) => (
                VaultState {
                    accounts: Vec::new(),
                    active_account_id: None,
                    settings,
                },
                mode,
                false,
                None,
            ),
            Err(err) => (
                VaultState::default(),
                EncryptionMode::DeviceKey,
                true,
                Some((err.to_string(), true)),
            ),
        };

        let mut svc = Self {
            repo,
            bridge,
            state,
            encryption_mode,
            unlocked,
            cached_passphrase: None,
            skins,
            job_rx: None,
            job_cancel: None,
            auth_job: AuthJobState::Idle,
            banner_message: banner,
        };

        // If previously selected account exists and vault unlocked cleanly, apply active state.
        if svc.unlocked
            && let Some(active_id) = svc.state.active_account_id.clone()
        {
            if svc.state.find(&active_id).is_some() {
                let _ = svc.switch_to(&active_id);
            } else {
                svc.state.active_account_id = None;
                let _ = svc.bridge.restore_launcher();
                let _ = svc.persist();
            }
        }
        svc
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked
    }

    pub fn encryption_mode(&self) -> EncryptionMode {
        self.encryption_mode
    }

    pub fn settings(&self) -> &ModSettings {
        &self.state.settings
    }

    pub fn active_account_id(&self) -> Option<&str> {
        self.state.active_account_id.as_deref()
    }

    /// Returns `(username, is_online)` reflecting either the switched session or initial game session.
    pub fn live_identity(&self) -> (String, bool) {
        self.bridge.current_identity()
    }

    /// Returns the original Horizon Launcher session info captured before in-game overrides.
    pub fn launcher_session(&self) -> Option<LauncherSessionInfo> {
        self.bridge.launcher_session()
    }

    /// Clears in-game overrides and switches Horizon back to the original launcher session.
    pub fn restore_launcher_session(&mut self) -> Result<(), SwitcherError> {
        self.bridge.restore_launcher()?;
        self.state.active_account_id = None;
        if self.unlocked {
            self.persist()?;
        }
        let (name, _) = self.bridge.current_identity();
        self.banner_message = Some((format!("Сессия лаунчера: {name}"), false));
        Ok(())
    }

    /// Returns cached 8x8 ARGB avatar pixels for the initial launcher session.
    pub fn launcher_avatar(&mut self, info: &LauncherSessionInfo) -> [u32; 64] {
        if info.is_online
            && let Some(uuid) = info.uuid
        {
            self.skins
                .ensure_requested("__launcher__", &format!("{uuid:032x}"), None);
        }
        self.skins
            .get_or_fallback("__launcher__", &info.username, info.is_online)
    }

    /// Unlocks a password-protected vault.
    pub fn unlock(&mut self, passphrase: &str) -> Result<(), SwitcherError> {
        match self.repo.load(Some(passphrase))? {
            LoadedVault::Unlocked { state, mode } => {
                self.state = state;
                self.encryption_mode = mode;
                self.unlocked = true;
                self.cached_passphrase = Some(passphrase.to_owned());
                for acc in &self.state.accounts {
                    if let Some(grid) = acc.profile().head_grid() {
                        self.skins.seed(acc.id(), grid);
                    }
                }
                self.banner_message = None;
                Ok(())
            }
            LoadedVault::Locked { .. } => Err(SwitcherError::LockedVault),
        }
    }

    /// Returns filtered & sorted accounts matching `query` and `filter`.
    pub fn visible_accounts(&self, query: &str, filter: AccountFilter) -> Vec<&AccountRecord> {
        let needle = query.trim().to_ascii_lowercase();
        let mut items: Vec<&AccountRecord> = self
            .state
            .accounts
            .iter()
            .filter(|acc| match filter {
                AccountFilter::All => true,
                AccountFilter::Microsoft => acc.kind() == AccountKind::Microsoft,
                AccountFilter::Offline => acc.kind() == AccountKind::Offline,
            })
            .filter(|acc| {
                needle.is_empty() || acc.username().to_ascii_lowercase().contains(&needle)
            })
            .collect();

        let active_id = self.state.active_account_id.as_deref();
        items.sort_by(|a, b| {
            let a_active = Some(a.id()) == active_id;
            let b_active = Some(b.id()) == active_id;
            b_active
                .cmp(&a_active)
                .then_with(|| b.profile().favorite.cmp(&a.profile().favorite))
                .then_with(|| b.profile().last_used_unix.cmp(&a.profile().last_used_unix))
                .then_with(|| a.username().cmp(b.username()))
        });
        items
    }

    /// Returns cached 8x8 ARGB avatar pixels or crisp fallback voxel head.
    pub fn avatar_for(&mut self, acc: &AccountRecord) -> [u32; 64] {
        if acc.is_online() {
            self.skins.ensure_requested(
                acc.id(),
                &format!("{:032x}", acc.uuid()),
                acc.profile().skin_url.clone(),
            );
        }
        self.skins
            .get_or_fallback(acc.id(), acc.username(), acc.is_online())
    }

    /// Adds a new offline account and saves the vault.
    pub fn add_offline_account(&mut self, username: &str, switch_now: bool) -> Result<String, SwitcherError> {
        if !self.unlocked {
            return Err(SwitcherError::LockedVault);
        }
        let id = new_account_id("off");
        let acc = OfflineAuthProvider::create_account(id.clone(), username)?;
        self.state.accounts.push(acc);
        if switch_now {
            self.switch_to(&id)?;
        } else {
            self.persist()?;
        }
        self.banner_message = Some((format!("Added offline profile: {}", username.trim()), false));
        Ok(id)
    }

    /// Switches Horizon's active runtime session to the specified account ID.
    pub fn switch_to(&mut self, account_id: &str) -> Result<(), SwitcherError> {
        if !self.unlocked {
            return Err(SwitcherError::LockedVault);
        }
        let now = unix_now();
        let idx = self
            .state
            .accounts
            .iter()
            .position(|a| a.id() == account_id)
            .ok_or_else(|| SwitcherError::Validation("Account not found".into()))?;

        enum Action {
            Offline(String),
            Online {
                username: String,
                uuid: u128,
                xuid: Option<String>,
                token: String,
                needs_refresh: bool,
                refresh_token: String,
            },
        }

        let action = match &self.state.accounts[idx] {
            AccountRecord::Offline(off) => Action::Offline(off.username.clone()),
            AccountRecord::Microsoft(ms) => {
                let expired = ms.status(now) == AuthStatusBadge::Expired;
                Action::Online {
                    username: ms.username.clone(),
                    uuid: ms.uuid(),
                    xuid: ms.tokens.xuid.clone(),
                    token: ms.tokens.mc_access_token.clone(),
                    needs_refresh: expired
                        && self.state.settings.auto_refresh_tokens
                        && !ms.tokens.ms_refresh_token.is_empty(),
                    refresh_token: ms.tokens.ms_refresh_token.clone(),
                }
            }
        };

        match action {
            Action::Offline(name) => {
                self.bridge.apply_offline(&name)?;
                self.state.accounts[idx].touch_last_used(now);
                self.state.active_account_id = Some(account_id.to_owned());
                self.persist()?;
                self.banner_message = Some((format!("Switched to offline: {name}"), false));
            }
            Action::Online {
                username,
                uuid,
                xuid,
                token,
                needs_refresh,
                refresh_token,
            } => {
                self.bridge
                    .apply_online(&username, uuid, xuid.as_deref(), &token)?;
                self.state.accounts[idx].touch_last_used(now);
                self.state.active_account_id = Some(account_id.to_owned());
                self.persist()?;
                self.banner_message = Some((format!("Switched to Microsoft: {username}"), false));

                if needs_refresh {
                    let _ = self.spawn_refresh(account_id, &refresh_token, true);
                }
            }
        }
        Ok(())
    }

    /// Toggles pinned favorite status for an account.
    pub fn toggle_favorite(&mut self, account_id: &str) -> Result<(), SwitcherError> {
        if let Some(acc) = self.state.find_mut(account_id) {
            acc.toggle_favorite();
            self.persist()?;
        }
        Ok(())
    }

    /// Deletes an account from the vault and restores launcher session if it was active or vault became empty.
    pub fn delete_account(&mut self, account_id: &str) -> Result<(), SwitcherError> {
        if let Some(pos) = self.state.accounts.iter().position(|a| a.id() == account_id) {
            let name = self.state.accounts[pos].username().to_owned();
            let was_active = self.state.active_account_id.as_deref() == Some(account_id);
            self.state.accounts.remove(pos);
            if was_active || self.state.accounts.is_empty() {
                self.state.active_account_id = None;
                self.bridge.restore_launcher()?;
            }
            self.persist()?;
            self.banner_message = Some((format!("Удалён профиль: {name}"), false));
        }
        Ok(())
    }

    /// Saves updated Client ID / UI settings & optionally re-encrypts the vault.
    pub fn update_security_and_settings(
        &mut self,
        new_mode: EncryptionMode,
        new_passphrase: Option<&str>,
        mut new_settings: ModSettings,
    ) -> Result<(), SwitcherError> {
        if !self.unlocked {
            return Err(SwitcherError::LockedVault);
        }
        if new_mode == EncryptionMode::PasswordProtected
            && new_passphrase.is_none_or(str::is_empty)
            && self.cached_passphrase.is_none()
        {
            return Err(SwitcherError::Validation(
                "Enter a non-empty master password".into(),
            ));
        }

        new_settings.custom_client_id = new_settings.custom_client_id.trim().to_owned();
        self.state.settings = new_settings;
        self.encryption_mode = new_mode;
        if let Some(pw) = new_passphrase.filter(|s| !s.is_empty()) {
            self.cached_passphrase = Some(pw.to_owned());
        } else if new_mode != EncryptionMode::PasswordProtected {
            self.cached_passphrase = None;
        }
        self.persist()?;
        self.banner_message = Some(("Settings & vault saved".into(), false));
        Ok(())
    }

    /// Launches a background OAuth login worker (Browser PKCE or Device Code).
    pub fn start_microsoft_login(
        &mut self,
        kind: OAuthLoginKind,
        client_id_override: &str,
    ) -> Result<(), SwitcherError> {
        if !self.unlocked {
            return Err(SwitcherError::LockedVault);
        }
        let cid = if !client_id_override.trim().is_empty() {
            client_id_override.trim().to_owned()
        } else {
            self.state.settings.custom_client_id.clone()
        };
        if cid != self.state.settings.custom_client_id && !cid.is_empty() {
            self.state.settings.custom_client_id = cid.clone();
            let _ = self.persist();
        }
        let provider = MicrosoftOAuthProvider::new(cid)?;
        if kind == OAuthLoginKind::BrowserPkce && !provider.uses_custom_azure_client() {
            return Err(SwitcherError::OAuth(
                "Для PKCE укажите Azure Client ID в Хранилище (или используйте «Код устройства»).".into(),
            ));
        }
        self.cancel_auth_job();

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        self.job_rx = Some(rx);
        self.job_cancel = Some(cancel);
        self.auth_job = AuthJobState::Running {
            stage: "Запуск входа через Microsoft...".into(),
            device_code: None,
            browser_url: None,
        };

        thread::Builder::new()
            .name("account-switcher-oauth".into())
            .spawn(move || {
                let notify = |ev: OAuthProgressEvent| {
                    let _ = tx.send(BackgroundJobMsg::OAuth(ev));
                };
                let res = match kind {
                    OAuthLoginKind::BrowserPkce => provider.login_browser_pkce(&cancel_worker, notify),
                    OAuthLoginKind::DeviceCode => provider.login_device_code(&cancel_worker, notify),
                };
                match res {
                    Ok(auth) => {
                        let _ = tx.send(BackgroundJobMsg::OAuth(OAuthProgressEvent::Completed(auth)));
                    }
                    Err(err) => {
                        let _ = tx.send(BackgroundJobMsg::OAuth(OAuthProgressEvent::Failed(
                            err.to_string(),
                        )));
                    }
                }
            })
            .map_err(|e| SwitcherError::Network(format!("Spawn auth thread: {e}")))?;
        Ok(())
    }

    /// Triggers manual token refresh on an existing Microsoft account.
    pub fn refresh_account(&mut self, account_id: &str) -> Result<(), SwitcherError> {
        let Some(acc) = self.state.find(account_id).and_then(|a| a.as_microsoft()) else {
            return Err(SwitcherError::Validation("Not a Microsoft account".into()));
        };
        if acc.tokens.ms_refresh_token.is_empty() {
            return Err(SwitcherError::OAuth("No refresh token stored".into()));
        }
        let rt = acc.tokens.ms_refresh_token.clone();
        self.spawn_refresh(account_id, &rt, self.state.active_account_id.as_deref() == Some(account_id))
    }

    fn spawn_refresh(
        &mut self,
        account_id: &str,
        refresh_token: &str,
        switch_after: bool,
    ) -> Result<(), SwitcherError> {
        let provider = MicrosoftOAuthProvider::new(self.state.settings.custom_client_id.clone())?;
        let (tx, rx) = mpsc::channel();
        self.job_rx = Some(rx);
        self.auth_job = AuthJobState::Running {
            stage: "Refreshing Microsoft session...".into(),
            device_code: None,
            browser_url: None,
        };
        let id_owned = account_id.to_owned();
        let rt_owned = refresh_token.to_owned();

        thread::Builder::new()
            .name("account-switcher-refresh".into())
            .spawn(move || {
                let res = provider
                    .refresh_tokens(&rt_owned)
                    .map_err(|e| e.to_string());
                let _ = tx.send(BackgroundJobMsg::RefreshComplete {
                    account_id: id_owned,
                    switch_after,
                    result: res,
                });
            })
            .map_err(|e| SwitcherError::Network(format!("Spawn refresh thread: {e}")))?;
        Ok(())
    }

    /// Cancels any active interactive OAuth thread cleanly.
    pub fn cancel_auth_job(&mut self) {
        if let Some(flag) = self.job_cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.job_rx = None;
        if matches!(self.auth_job, AuthJobState::Running { .. }) {
            self.auth_job = AuthJobState::Idle;
        }
    }

    /// Polls background skin & OAuth worker queues; call once per frame or tick.
    pub fn poll_background(&mut self) {
        // 1. Drain completed skin avatars and persist metadata if changed.
        let ready_skins = self.skins.poll_ready();
        if !ready_skins.is_empty() && self.unlocked {
            let mut changed = false;
            for item in ready_skins {
                if let Some(acc) = self.state.find_mut(&item.account_id) {
                    acc.profile_mut().set_head_grid(item.head_argb);
                    if let Some(url) = item.skin_url {
                        acc.profile_mut().skin_url = Some(url);
                    }
                    changed = true;
                }
            }
            if changed {
                let _ = self.persist();
            }
        }

        // 2. Drain background OAuth / refresh channel messages.
        let mut msgs = Vec::new();
        if let Some(rx) = &self.job_rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        for msg in msgs {
            match msg {
                BackgroundJobMsg::OAuth(ev) => match ev {
                    OAuthProgressEvent::Stage(stage) => {
                        if let AuthJobState::Running {
                            device_code,
                            browser_url,
                            ..
                        } = &self.auth_job
                        {
                            self.auth_job = AuthJobState::Running {
                                stage,
                                device_code: device_code.clone(),
                                browser_url: browser_url.clone(),
                            };
                        }
                    }
                    OAuthProgressEvent::DeviceCodeReady(dc) => {
                        self.auth_job = AuthJobState::Running {
                            stage: "Введите код на открывшейся странице...".into(),
                            device_code: Some(dc),
                            browser_url: None,
                        };
                    }
                    OAuthProgressEvent::BrowserUrlReady(url) => {
                        self.auth_job = AuthJobState::Running {
                            stage: "Ожидание входа Microsoft в браузере...".into(),
                            device_code: None,
                            browser_url: Some(url),
                        };
                    }
                    OAuthProgressEvent::Completed(auth) => {
                        self.job_rx = None;
                        self.job_cancel = None;
                        let name = auth.username.clone();
                        match self.upsert_microsoft_account(auth) {
                            Ok(id) => {
                                let _ = self.switch_to(&id);
                                self.auth_job = AuthJobState::Succeeded(name);
                            }
                            Err(e) => {
                                self.auth_job = AuthJobState::Failed(e.to_string());
                            }
                        }
                    }
                    OAuthProgressEvent::Failed(err) => {
                        self.job_rx = None;
                        self.job_cancel = None;
                        self.auth_job = AuthJobState::Failed(err);
                    }
                },
                BackgroundJobMsg::RefreshComplete {
                    account_id,
                    switch_after,
                    result,
                } => {
                    self.job_rx = None;
                    match result {
                        Ok(res) => {
                            let name = res.username.clone();
                            if let Some(ms) = self
                                .state
                                .find_mut(&account_id)
                                .and_then(|a| a.as_microsoft_mut())
                                && ms.update_tokens(res.username, res.tokens).is_ok()
                            {
                                ms.profile.skin_url = res.skin_url;
                                let _ = self.persist();
                                if switch_after {
                                    let _ = self.switch_to(&account_id);
                                }
                                self.auth_job = AuthJobState::Succeeded(name);
                                self.banner_message =
                                    Some(("Refreshed Microsoft session".into(), false));
                            }
                        }
                        Err(e) => {
                            self.auth_job = AuthJobState::Failed(e.clone());
                            self.banner_message = Some((e, true));
                        }
                    }
                }
            }
        }
    }

    fn upsert_microsoft_account(
        &mut self,
        res: crate::auth::MicrosoftAuthResult,
    ) -> Result<String, SwitcherError> {
        let uuid_hex = crate::domain::format_uuid_undashed(res.uuid);
        if let Some(existing) = self
            .state
            .accounts
            .iter_mut()
            .find(|a| a.is_online() && a.uuid() == res.uuid)
        {
            let existing_id = existing.id().to_owned();
            if let Some(ms) = existing.as_microsoft_mut() {
                ms.update_tokens(res.username, res.tokens)?;
                ms.profile.skin_url = res.skin_url.clone();
                ms.profile.last_used_unix = unix_now();
            }
            self.skins
                .ensure_requested(&existing_id, &uuid_hex, res.skin_url);
            self.persist()?;
            return Ok(existing_id);
        }

        let id = new_account_id("ms");
        let skin = res.skin_url.clone();
        let rec = res.into_account(id.clone())?;
        self.state.accounts.push(rec);
        self.skins.ensure_requested(&id, &uuid_hex, skin);
        self.persist()?;
        Ok(id)
    }

    fn persist(&self) -> Result<(), SwitcherError> {
        self.repo.save(
            &self.state,
            self.encryption_mode,
            self.cached_passphrase.as_deref(),
        )
    }
}

fn new_account_id(prefix: &str) -> String {
    let bytes = random_bytes(6).unwrap_or_else(|_| vec![1, 2, 3, 4, 5, 6]);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}_{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FileAccountRepository;
    use std::sync::Mutex;

    struct FakeBridge {
        launcher: LauncherSessionInfo,
        state: Mutex<(String, bool)>,
    }

    impl FakeBridge {
        fn new() -> Self {
            Self {
                launcher: LauncherSessionInfo {
                    username: "LauncherPro".into(),
                    is_online: true,
                    uuid: Some(0x112233445566778899aabbccddeeff00),
                },
                state: Mutex::new(("LauncherPro".into(), true)),
            }
        }
    }

    impl SessionBridge for FakeBridge {
        fn apply_online(
            &self,
            username: &str,
            _uuid: u128,
            _xuid: Option<&str>,
            _access_token: &str,
        ) -> Result<(), SwitcherError> {
            *self.state.lock().unwrap() = (username.to_owned(), true);
            Ok(())
        }

        fn apply_offline(&self, username: &str) -> Result<(), SwitcherError> {
            *self.state.lock().unwrap() = (username.to_owned(), false);
            Ok(())
        }

        fn restore_launcher(&self) -> Result<(), SwitcherError> {
            *self.state.lock().unwrap() = (self.launcher.username.clone(), self.launcher.is_online);
            Ok(())
        }

        fn current_identity(&self) -> (String, bool) {
            self.state.lock().unwrap().clone()
        }

        fn launcher_session(&self) -> Option<LauncherSessionInfo> {
            Some(self.launcher.clone())
        }
    }

    #[test]
    fn service_adds_switches_filters_and_restores_launcher_on_delete() {
        let dir = std::env::temp_dir().join(format!("horizon_as_svc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = Arc::new(FakeBridge::new());
        let repo = Box::new(FileAccountRepository::in_dir(&dir));
        let mut svc = AccountSwitcherService::bootstrap(repo, bridge.clone());
        assert_eq!(svc.live_identity(), ("LauncherPro".into(), true));

        let id1 = svc.add_offline_account("SteveOne", true).unwrap();
        let id2 = svc.add_offline_account("AlexTwo", false).unwrap();
        assert_eq!(svc.live_identity(), ("SteveOne".into(), false));

        // Explicit restore returns to launcher online account
        svc.restore_launcher_session().unwrap();
        assert_eq!(svc.live_identity(), ("LauncherPro".into(), true));
        assert_eq!(svc.active_account_id(), None);

        svc.switch_to(&id2).unwrap();
        assert_eq!(svc.live_identity(), ("AlexTwo".into(), false));

        assert_eq!(svc.visible_accounts("alex", AccountFilter::All).len(), 1);
        assert_eq!(svc.visible_accounts("", AccountFilter::Microsoft).len(), 0);

        svc.toggle_favorite(&id1).unwrap();
        let sorted = svc.visible_accounts("", AccountFilter::All);
        assert_eq!(sorted[0].username(), "AlexTwo"); // Active stays first
        assert!(sorted[1].profile().favorite);

        // Deleting non-active leaves AlexTwo active; deleting active rolls back to launcher session
        svc.delete_account(&id1).unwrap();
        assert_eq!(svc.live_identity(), ("AlexTwo".into(), false));
        svc.delete_account(&id2).unwrap();
        assert_eq!(svc.live_identity(), ("LauncherPro".into(), true));
        assert_eq!(svc.active_account_id(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
