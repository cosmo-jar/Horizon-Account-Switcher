// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

pub mod md5;
pub mod microsoft;
pub mod offline;
pub mod skins;

pub use microsoft::{
    DeviceCodePrompt, MicrosoftAuthResult, MicrosoftOAuthProvider, OAuthLoginKind,
    OAuthProgressEvent, parse_entitlements_json, parse_mc_login_json, parse_mc_profile_json,
    parse_ms_token_json, parse_xbox_token_json,
};
pub use offline::{OfflineAuthProvider, offline_uuid_for};
pub use skins::{SkinAvatarCache, composite_skin_head_png, fallback_avatar};
