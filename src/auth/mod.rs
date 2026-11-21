// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use crate::domain::SwitcherError;

pub mod md5;
pub mod microsoft;
pub mod offline;
pub mod skins;

/// Raw HTTP response returned by `HttpBridge`.
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Transport bridge allowing the host game crate's prebuilt HTTPS stack (`ureq` / `rustls`)
/// to serve OAuth and Mojang skin requests without bundling a duplicate `ring` native library.
pub trait HttpBridge: Send + Sync {
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<HttpResponse, SwitcherError>;
    fn post_json(&self, url: &str, json_body: &str) -> Result<HttpResponse, SwitcherError>;
    fn get_text(&self, url: &str, bearer: Option<&str>) -> Result<String, SwitcherError>;
    fn get_bytes(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, SwitcherError>;
}

/// Fallback no-op transport used only in isolated unit tests without network I/O.
pub struct NoopHttpBridge;

impl HttpBridge for NoopHttpBridge {
    fn post_form(&self, _: &str, _: &[(&str, &str)]) -> Result<HttpResponse, SwitcherError> {
        Err(SwitcherError::Network("HTTP bridge unavailable in offline test mode".into()))
    }

    fn post_json(&self, _: &str, _: &str) -> Result<HttpResponse, SwitcherError> {
        Err(SwitcherError::Network("HTTP bridge unavailable in offline test mode".into()))
    }

    fn get_text(&self, _: &str, _: Option<&str>) -> Result<String, SwitcherError> {
        Err(SwitcherError::Network("HTTP bridge unavailable in offline test mode".into()))
    }

    fn get_bytes(&self, _: &str, _: usize) -> Result<Vec<u8>, SwitcherError> {
        Err(SwitcherError::Network("HTTP bridge unavailable in offline test mode".into()))
    }
}

pub use microsoft::{
    DeviceCodePrompt, MicrosoftAuthResult, MicrosoftOAuthProvider, OAuthLoginKind,
    OAuthProgressEvent, parse_entitlements_json, parse_mc_login_json, parse_mc_profile_json,
    parse_ms_token_json, parse_xbox_token_json,
};
pub use offline::{OfflineAuthProvider, offline_uuid_for};
pub use skins::{SkinAvatarCache, composite_skin_head_png, fallback_avatar};
