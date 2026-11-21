// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    auth::{HttpBridge, NoopHttpBridge},
    crypto::cipher::random_bytes,
    domain::{
        AccountRecord, MicrosoftAccount, MicrosoftTokens, SwitcherError, account::unix_now,
        parse_uuid_any, validate_minecraft_name,
    },
};

const MS_AUTH_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const MS_SCOPE: &str = "XboxLive.signin offline_access";
const LIVE_CONNECT_URL: &str = "https://login.live.com/oauth20_connect.srf";
const LIVE_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const LIVE_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
pub const DEFAULT_LIVE_CLIENT_ID: &str = "00000000441cc96b";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_XBOX_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// Interactive Microsoft authentication method chosen in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OAuthLoginKind {
    BrowserPkce,
    DeviceCode,
}

/// Device code challenge handed to the UI while polling in the background.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceCodePrompt {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_secs: u64,
}

/// Progress updates streamed asynchronously from the OAuth background worker to the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OAuthProgressEvent {
    Stage(String),
    DeviceCodeReady(DeviceCodePrompt),
    BrowserUrlReady(String),
    Completed(MicrosoftAuthResult),
    Failed(String),
}

/// Complete verified Minecraft identity + tokens produced by `MicrosoftOAuthProvider`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicrosoftAuthResult {
    pub username: String,
    pub uuid: u128,
    pub tokens: MicrosoftTokens,
    pub skin_url: Option<String>,
}

impl MicrosoftAuthResult {
    /// Materializes a persistent `AccountRecord::Microsoft` entity.
    pub fn into_account(self, id: String) -> Result<AccountRecord, SwitcherError> {
        let mut acc = MicrosoftAccount::new(id, self.username, self.uuid, self.tokens, unix_now())?;
        acc.profile.skin_url = self.skin_url;
        Ok(AccountRecord::Microsoft(acc))
    }
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    #[serde(alias = "verification_url")]
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
pub struct MsTokenPayload {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct XboxAuthPayload {
    pub token: String,
    pub user_hash: String,
    pub xuid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct McLoginPayload {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct McProfilePayload {
    pub uuid: u128,
    pub name: String,
    pub skin_url: Option<String>,
}

/// Handles Microsoft OAuth 2.0 (Device Code + optional Browser PKCE + Refresh), Xbox Live, XSTS, and MC Services.
pub struct MicrosoftOAuthProvider {
    http: Arc<dyn HttpBridge>,
    client_id: String,
    uses_default_live: bool,
}

impl MicrosoftOAuthProvider {
    /// Configures a provider backed by an offline no-op bridge (for unit tests).
    pub fn new(client_id: String) -> Result<Self, SwitcherError> {
        Self::with_http(client_id, Arc::new(NoopHttpBridge))
    }

    /// Configures an HTTP-backed provider. Falls back to `HORIZON_MSA_CLIENT_ID` or the built-in public Live client ID.
    pub fn with_http(client_id: String, http: Arc<dyn HttpBridge>) -> Result<Self, SwitcherError> {
        let trimmed = client_id.trim().to_owned();
        let (resolved, uses_default_live) = if !trimmed.is_empty() {
            (trimmed, false)
        } else if let Ok(env_id) = std::env::var("HORIZON_MSA_CLIENT_ID")
            && !env_id.trim().is_empty()
        {
            (env_id.trim().to_owned(), false)
        } else {
            (DEFAULT_LIVE_CLIENT_ID.to_owned(), true)
        };
        Ok(Self {
            http,
            client_id: resolved,
            uses_default_live,
        })
    }

    pub fn uses_custom_azure_client(&self) -> bool {
        !self.uses_default_live
    }

    /// Runs interactive Device Code login until verified, timed out, or canceled.
    pub fn login_device_code(
        &self,
        cancel: &Arc<AtomicBool>,
        notify: impl Fn(OAuthProgressEvent),
    ) -> Result<MicrosoftAuthResult, SwitcherError> {
        notify(OAuthProgressEvent::Stage("Запрос кода Microsoft...".into()));
        let resp = if self.uses_default_live {
            let form = [
                ("client_id", self.client_id.as_str()),
                ("scope", LIVE_SCOPE),
                ("response_type", "device_code"),
            ];
            self.http.post_form(LIVE_CONNECT_URL, &form)?
        } else {
            let form = [("client_id", self.client_id.as_str()), ("scope", MS_SCOPE)];
            self.http
                .post_form(&format!("{MS_AUTH_BASE}/devicecode"), &form)?
        };
        if !(200..300).contains(&resp.status) {
            return Err(SwitcherError::Network(format!(
                "Запрос кода не удался (HTTP {})",
                resp.status
            )));
        }
        let text = resp.body;
        let dc: DeviceCodeResponse = serde_json::from_str(&text)
            .map_err(|e| SwitcherError::OAuth(format!("Некорректный JSON кода: {e}")))?;

        let _ = open::that_detached(&dc.verification_uri);
        notify(OAuthProgressEvent::DeviceCodeReady(DeviceCodePrompt {
            user_code: dc.user_code.clone(),
            verification_uri: dc.verification_uri.clone(),
            expires_in_secs: dc.expires_in,
        }));

        let deadline = Instant::now() + Duration::from_secs(dc.expires_in.min(900));
        let poll_wait = Duration::from_secs(dc.interval.max(3));
        let token_url = if self.uses_default_live {
            LIVE_TOKEN_URL.to_owned()
        } else {
            format!("{MS_AUTH_BASE}/token")
        };

        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Err(SwitcherError::OAuth("Вход через Microsoft отменён".into()));
            }
            std::thread::sleep(poll_wait);
            if cancel.load(Ordering::Relaxed) {
                return Err(SwitcherError::OAuth("Вход через Microsoft отменён".into()));
            }

            let poll_form = [
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", self.client_id.as_str()),
                ("device_code", dc.device_code.as_str()),
            ];
            let resp = self.http.post_form(&token_url, &poll_form)?;
            if (200..300).contains(&resp.status) {
                let ms = parse_ms_token_json(&resp.body)?;
                return self.complete_xbox_to_minecraft(ms, &notify);
            }
            let v: serde_json::Value = serde_json::from_str(&resp.body).unwrap_or_default();
            let err_code = v.get("error").and_then(|s| s.as_str()).unwrap_or("");
            match err_code {
                "authorization_pending" | "slow_down" => continue,
                "authorization_declined" => {
                    return Err(SwitcherError::OAuth("Вход отклонён пользователем".into()));
                }
                "expired_token" => {
                    return Err(SwitcherError::OAuth("Срок действия кода истёк".into()));
                }
                _ => {
                    return Err(SwitcherError::OAuth(format!(
                        "Ошибка OAuth ({}): {err_code}",
                        resp.status
                    )));
                }
            }
        }
        Err(SwitcherError::OAuth("Время ожидания кода истекло".into()))
    }

    /// Runs interactive Browser OAuth 2.0 Authorization Code + PKCE (S256) flow over localhost.
    pub fn login_browser_pkce(
        &self,
        cancel: &Arc<AtomicBool>,
        notify: impl Fn(OAuthProgressEvent),
    ) -> Result<MicrosoftAuthResult, SwitcherError> {
        if self.uses_default_live {
            return Err(SwitcherError::OAuth(
                "Для PKCE нужен Azure Client ID (или выберите «Код устройства» — работает без настроек).".into(),
            ));
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| SwitcherError::Network(format!("Bind loopback callback: {e}")))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| SwitcherError::Network(format!("Configure loopback socket: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| SwitcherError::Network(format!("Inspect loopback port: {e}")))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");

        let verifier_bytes = random_bytes(32)?;
        let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = URL_SAFE_NO_PAD.encode(random_bytes(16)?);

        let authorize_url = format!(
            "{MS_AUTH_BASE}/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&prompt=select_account",
            url_encode(&self.client_id),
            url_encode(&redirect_uri),
            url_encode(MS_SCOPE),
            url_encode(&challenge),
            url_encode(&state),
        );

        notify(OAuthProgressEvent::BrowserUrlReady(authorize_url.clone()));
        notify(OAuthProgressEvent::Stage(
            "Waiting for browser authentication...".into(),
        ));
        let _ = open::that_detached(&authorize_url);

        let code = wait_loopback_code(&listener, &state, cancel, Duration::from_secs(300))?;
        notify(OAuthProgressEvent::Stage(
            "Exchanging authorization code...".into(),
        ));

        let form = [
            ("client_id", self.client_id.as_str()),
            ("scope", MS_SCOPE),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ];
        let resp = self
            .http
            .post_form(&format!("{MS_AUTH_BASE}/token"), &form)?;
        if !(200..300).contains(&resp.status) {
            return Err(SwitcherError::OAuth(format!(
                "Token exchange failed (HTTP {})",
                resp.status
            )));
        }
        let ms = parse_ms_token_json(&resp.body)?;
        self.complete_xbox_to_minecraft(ms, &notify)
    }

    /// Silently exchanges a saved Microsoft refresh token for a fresh Minecraft session.
    pub fn refresh_tokens(&self, refresh_token: &str) -> Result<MicrosoftAuthResult, SwitcherError> {
        let (token_url, scope) = if self.uses_default_live {
            (LIVE_TOKEN_URL.to_owned(), LIVE_SCOPE)
        } else {
            (format!("{MS_AUTH_BASE}/token"), MS_SCOPE)
        };
        let form = [
            ("client_id", self.client_id.as_str()),
            ("scope", scope),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];
        let resp = self.http.post_form(&token_url, &form)?;
        if !(200..300).contains(&resp.status) {
            return Err(SwitcherError::OAuth(format!(
                "Обновление токена не удалось (HTTP {})",
                resp.status
            )));
        }
        let mut ms = parse_ms_token_json(&resp.body)?;
        if ms.refresh_token.is_none() {
            ms.refresh_token = Some(refresh_token.to_owned());
        }
        self.complete_xbox_to_minecraft(ms, &|_| {})
    }

    fn complete_xbox_to_minecraft(
        &self,
        ms: MsTokenPayload,
        notify: &impl Fn(OAuthProgressEvent),
    ) -> Result<MicrosoftAuthResult, SwitcherError> {
        notify(OAuthProgressEvent::Stage("Авторизация Xbox Live...".into()));
        let rps_ticket = if self.uses_default_live {
            format!("t={}", ms.access_token)
        } else {
            format!("d={}", ms.access_token)
        };
        let xbl_payload = serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": rps_ticket,
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        });
        let xbl_body = post_json(&*self.http, XBL_URL, &xbl_payload)
            .map_err(|e| SwitcherError::OAuth(format!("Ошибка Xbox Live: {e}")))?;
        let xbl = parse_xbox_token_json(&xbl_body)?;

        notify(OAuthProgressEvent::Stage("Авторизация XSTS...".into()));
        let xsts_payload = serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl.token],
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        });
        let xsts_resp = self
            .http
            .post_json(XSTS_URL, &xsts_payload.to_string())?;
        let xsts_body = if (200..300).contains(&xsts_resp.status) {
            xsts_resp.body
        } else if xsts_resp.status == 401 {
            return Err(SwitcherError::OAuth(explain_xsts_error(&xsts_resp.body)));
        } else {
            return Err(SwitcherError::OAuth(format!(
                "XSTS request failed (HTTP {})",
                xsts_resp.status
            )));
        };
        let xsts = parse_xbox_token_json(&xsts_body)?;
        let uhs = if xsts.user_hash.is_empty() {
            xbl.user_hash
        } else {
            xsts.user_hash
        };

        notify(OAuthProgressEvent::Stage(
            "Fetching Minecraft Java token...".into(),
        ));
        let mc_login_body = post_json(
            &*self.http,
            MC_XBOX_URL,
            &serde_json::json!({
                "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token)
            }),
        )
        .map_err(|e| SwitcherError::OAuth(format!("Minecraft login error: {e}")))?;
        let mc_login = parse_mc_login_json(&mc_login_body)?;

        notify(OAuthProgressEvent::Stage("Verifying game ownership...".into()));
        let ent_body = self
            .http
            .get_text(MC_ENTITLEMENTS_URL, Some(&mc_login.access_token))
            .map_err(|e| SwitcherError::OAuth(format!("Ownership check failed: {e}")))?;
        let owned = parse_entitlements_json(&ent_body)?;
        if !owned {
            return Err(SwitcherError::OAuth(
                "This Microsoft account does not own Minecraft: Java Edition".into(),
            ));
        }

        notify(OAuthProgressEvent::Stage("Loading Minecraft profile...".into()));
        let profile_body = self
            .http
            .get_text(MC_PROFILE_URL, Some(&mc_login.access_token))
            .map_err(|e| SwitcherError::OAuth(format!("Profile fetch failed: {e}")))?;
        let profile = parse_mc_profile_json(&profile_body)?;

        let expires_at_unix = unix_now().saturating_add(mc_login.expires_in);
        Ok(MicrosoftAuthResult {
            username: profile.name,
            uuid: profile.uuid,
            tokens: MicrosoftTokens {
                mc_access_token: mc_login.access_token,
                ms_refresh_token: ms.refresh_token.unwrap_or_default(),
                expires_at_unix,
                xuid: xsts.xuid,
                ownership_verified: true,
            },
            skin_url: profile.skin_url,
        })
    }
}

fn wait_loopback_code(
    listener: &TcpListener,
    expected_state: &str,
    cancel: &Arc<AtomicBool>,
    timeout: Duration,
) -> Result<String, SwitcherError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return Err(SwitcherError::OAuth("Browser sign-in canceled".into()));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buf = [0u8; 4096];
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let parsed = extract_oauth_callback_query(&req, expected_state);
                let html = match &parsed {
                    Ok(_) => "<html><body style='background:#0a0919;color:#fff;font-family:sans-serif;text-align:center;padding-top:48px'><h2>Signed in to Horizon Account Switcher</h2><p>You can close this tab and return to the game.</p></body></html>",
                    Err(err) => {
                        let _ = err;
                        "<html><body style='background:#0a0919;color:#ff8a8a;font-family:sans-serif;text-align:center;padding-top:48px'><h2>Authentication did not complete</h2><p>Check the game window for details.</p></body></html>"
                    }
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
                return parsed;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(80));
            }
            Err(e) => return Err(SwitcherError::Network(format!("Callback socket error: {e}"))),
        }
    }
    Err(SwitcherError::OAuth("Timed out waiting for browser callback".into()))
}

/// Parses an HTTP `GET /callback?code=...&state=...` request line from loopback OAuth.
pub fn extract_oauth_callback_query(
    http_request: &str,
    expected_state: &str,
) -> Result<String, SwitcherError> {
    let first_line = http_request.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| SwitcherError::OAuth("Malformed loopback request".into()))?;
    let query = path
        .split_once('?')
        .map(|(_, q)| q)
        .ok_or_else(|| SwitcherError::OAuth("Missing OAuth callback query".into()))?;

    let mut code = None;
    let mut state = None;
    let mut err = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let val = url_decode(v);
        match k {
            "code" => code = Some(val),
            "state" => state = Some(val),
            "error" | "error_description" => {
                if err.is_none() || k == "error_description" {
                    err = Some(val);
                }
            }
            _ => {}
        }
    }
    if let Some(msg) = err {
        return Err(SwitcherError::OAuth(format!("Microsoft returned: {msg}")));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(SwitcherError::OAuth("OAuth CSRF state mismatch".into()));
    }
    code.filter(|c| !c.is_empty())
        .ok_or_else(|| SwitcherError::OAuth("Authorization code missing in callback".into()))
}

/// Parses Microsoft OAuth `/token` JSON payload.
pub fn parse_ms_token_json(body: &str) -> Result<MsTokenPayload, SwitcherError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| SwitcherError::OAuth(format!("Invalid MS token JSON: {e}")))?;
    let access_token = v
        .get("access_token")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SwitcherError::OAuth("Microsoft token missing access_token".into()))?
        .to_owned();
    let refresh_token = v
        .get("refresh_token")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Ok(MsTokenPayload {
        access_token,
        refresh_token,
    })
}

/// Parses Xbox Live / XSTS authentication JSON payload.
pub fn parse_xbox_token_json(body: &str) -> Result<XboxAuthPayload, SwitcherError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| SwitcherError::OAuth(format!("Invalid Xbox JSON: {e}")))?;
    let token = v
        .get("Token")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SwitcherError::OAuth("Xbox response missing Token".into()))?
        .to_owned();
    let xui = v
        .pointer("/DisplayClaims/xui/0")
        .ok_or_else(|| SwitcherError::OAuth("Xbox response missing DisplayClaims.xui[0]".into()))?;
    let user_hash = xui
        .get("uhs")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_owned();
    let xuid = xui.get("xid").and_then(|s| s.as_str()).map(str::to_owned);
    Ok(XboxAuthPayload {
        token,
        user_hash,
        xuid,
    })
}

/// Parses Minecraft `login_with_xbox` JSON response.
pub fn parse_mc_login_json(body: &str) -> Result<McLoginPayload, SwitcherError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| SwitcherError::OAuth(format!("Invalid MC login JSON: {e}")))?;
    let access_token = v
        .get("access_token")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SwitcherError::OAuth("MC login missing access_token".into()))?
        .to_owned();
    let expires_in = v.get("expires_in").and_then(|n| n.as_u64()).unwrap_or(86_400);
    Ok(McLoginPayload {
        access_token,
        expires_in,
    })
}

/// Parses `/entitlements/mcstore` JSON to confirm Minecraft ownership.
pub fn parse_entitlements_json(body: &str) -> Result<bool, SwitcherError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| SwitcherError::OAuth(format!("Invalid entitlements JSON: {e}")))?;
    let items = v
        .get("items")
        .and_then(|i| i.as_array())
        .ok_or_else(|| SwitcherError::OAuth("Entitlements missing items array".into()))?;
    Ok(items.iter().any(|item| {
        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
        name == "product_minecraft" || name == "game_minecraft"
    }))
}

/// Parses `/minecraft/profile` JSON response including active skin URL.
pub fn parse_mc_profile_json(body: &str) -> Result<McProfilePayload, SwitcherError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| SwitcherError::OAuth(format!("Invalid MC profile JSON: {e}")))?;
    let uuid = v
        .get("id")
        .and_then(|s| s.as_str())
        .and_then(parse_uuid_any)
        .ok_or_else(|| SwitcherError::OAuth("MC profile missing valid id".into()))?;
    let name = v
        .get("name")
        .and_then(|s| s.as_str())
        .ok_or_else(|| SwitcherError::OAuth("MC profile missing name".into()))?
        .to_owned();
    validate_minecraft_name(&name)?;
    let skin_url = v
        .get("skins")
        .and_then(|s| s.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|entry| entry.get("state").and_then(|st| st.as_str()) == Some("ACTIVE"))
                .or_else(|| arr.first())
        })
        .and_then(|skin| skin.get("url").and_then(|u| u.as_str()))
        .map(str::to_owned);
    Ok(McProfilePayload {
        uuid,
        name,
        skin_url,
    })
}

fn post_json(
    http: &dyn HttpBridge,
    url: &str,
    payload: &serde_json::Value,
) -> Result<String, SwitcherError> {
    let resp = http.post_json(url, &payload.to_string())?;
    if (200..300).contains(&resp.status) {
        Ok(resp.body)
    } else {
        Err(SwitcherError::OAuth(format!(
            "HTTP JSON post failed (HTTP {})",
            resp.status
        )))
    }
}

fn explain_xsts_error(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    match v.get("XErr").and_then(|n| n.as_u64()) {
        Some(2148916233) => "No Xbox Live profile linked to this Microsoft account".into(),
        Some(2148916235) => "Xbox Live is unavailable in this account's region".into(),
        Some(2148916238) => "Child/under-18 account must join a Microsoft Family".into(),
        Some(code) => format!("XSTS authorization denied (code {code})"),
        None => "XSTS authorization rejected".into(),
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && let Ok(hex) = std::str::from_utf8(&raw[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex, 16)
        {
            bytes.push(val);
            i += 3;
            continue;
        }
        bytes.push(if raw[i] == b'+' { b' ' } else { raw[i] });
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xbox_mc_entitlements_and_profile_payloads() {
        let xbl = r#"{"Token":"xbl_tok","DisplayClaims":{"xui":[{"uhs":"userhash1","xid":"123456"}]}}"#;
        let parsed_xbl = parse_xbox_token_json(xbl).unwrap();
        assert_eq!(parsed_xbl.token, "xbl_tok");
        assert_eq!(parsed_xbl.user_hash, "userhash1");
        assert_eq!(parsed_xbl.xuid.as_deref(), Some("123456"));

        let mc = r#"{"access_token":"mc_jwt","expires_in":43200}"#;
        let parsed_mc = parse_mc_login_json(mc).unwrap();
        assert_eq!(parsed_mc.access_token, "mc_jwt");
        assert_eq!(parsed_mc.expires_in, 43200);

        let ent = r#"{"items":[{"name":"product_minecraft"},{"name":"game_minecraft"}]}"#;
        assert!(parse_entitlements_json(ent).unwrap());

        let prof = r#"{"id":"069a79f444e94726a5befca90e38aaf5","name":"Notch","skins":[{"state":"ACTIVE","url":"https://textures.minecraft.net/texture/abc"}]}"#;
        let parsed_prof = parse_mc_profile_json(prof).unwrap();
        assert_eq!(parsed_prof.name, "Notch");
        assert_eq!(
            parsed_prof.skin_url.as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
    }

    #[test]
    fn extracts_oauth_callback_query_and_checks_state() {
        let req = "GET /callback?code=auth_123&state=st_abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            extract_oauth_callback_query(req, "st_abc").unwrap(),
            "auth_123"
        );
        assert!(extract_oauth_callback_query(req, "wrong").is_err());
    }

    #[test]
    fn defaults_to_builtin_public_live_client_when_custom_empty() {
        let provider = MicrosoftOAuthProvider::new(String::new()).unwrap();
        assert!(!provider.uses_custom_azure_client());
        let custom = MicrosoftOAuthProvider::new("11111111-2222-3333-4444-555555555555".into()).unwrap();
        assert!(custom.uses_custom_azure_client());
    }
}
