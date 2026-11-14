// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::SwitcherError;

/// Discriminant distinguishing authenticated Microsoft profiles from offline ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Microsoft,
    Offline,
}

/// Live credential health indicator rendered on account cards and pills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthStatusBadge {
    Valid,
    ExpiringSoon,
    Expired,
    Offline,
}

/// Token material held for a Microsoft-authenticated Minecraft account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrosoftTokens {
    pub mc_access_token: String,
    pub ms_refresh_token: String,
    pub expires_at_unix: u64,
    pub xuid: Option<String>,
    pub ownership_verified: bool,
}

/// Mutable metadata and cached 8x8 composited skin avatar shared across account variants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    pub skin_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_head_argb: Option<Vec<u32>>,
    pub favorite: bool,
    pub last_used_unix: u64,
    pub created_unix: u64,
}

impl AccountProfile {
    /// Constructs a new profile timestamped at creation.
    pub fn with_created(now_unix: u64) -> Self {
        Self {
            skin_url: None,
            cached_head_argb: None,
            favorite: false,
            last_used_unix: now_unix,
            created_unix: now_unix,
        }
    }

    /// Returns an `8x8` ARGB skin pixel array if a valid 64-pixel avatar has been cached.
    pub fn head_grid(&self) -> Option<[u32; 64]> {
        let slice = self.cached_head_argb.as_deref()?;
        if slice.len() == 64 {
            let mut out = [0u32; 64];
            out.copy_from_slice(slice);
            Some(out)
        } else {
            None
        }
    }

    /// Stores a composited `8x8` ARGB avatar grid.
    pub fn set_head_grid(&mut self, grid: [u32; 64]) {
        self.cached_head_argb = Some(grid.to_vec());
    }
}

/// Polymorphic account contract shared by Microsoft and offline account entities.
pub trait AccountIdentity {
    fn id(&self) -> &str;
    fn kind(&self) -> AccountKind;
    fn username(&self) -> &str;
    fn uuid(&self) -> u128;
    fn profile(&self) -> &AccountProfile;
    fn status(&self, now_unix: u64) -> AuthStatusBadge;

    fn is_online(&self) -> bool {
        self.kind() == AccountKind::Microsoft
    }

    fn uuid_dashed(&self) -> String {
        format_uuid_dashed(self.uuid())
    }
}

/// Authenticated Microsoft/Xbox/Minecraft account entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrosoftAccount {
    pub id: String,
    pub username: String,
    pub uuid_hex: String,
    pub tokens: MicrosoftTokens,
    pub profile: AccountProfile,
}

impl MicrosoftAccount {
    /// Builds a validated Microsoft account entity.
    pub fn new(
        id: String,
        username: String,
        uuid: u128,
        tokens: MicrosoftTokens,
        now_unix: u64,
    ) -> Result<Self, SwitcherError> {
        validate_minecraft_name(&username)?;
        Ok(Self {
            id,
            username,
            uuid_hex: format_uuid_undashed(uuid),
            tokens,
            profile: AccountProfile::with_created(now_unix),
        })
    }

    /// Replaces tokens after an OAuth/XSTS/MC refresh cycle.
    pub fn update_tokens(&mut self, username: String, tokens: MicrosoftTokens) -> Result<(), SwitcherError> {
        validate_minecraft_name(&username)?;
        self.username = username;
        self.tokens = tokens;
        Ok(())
    }
}

impl AccountIdentity for MicrosoftAccount {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> AccountKind {
        AccountKind::Microsoft
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn uuid(&self) -> u128 {
        parse_uuid_any(&self.uuid_hex).unwrap_or(0)
    }

    fn profile(&self) -> &AccountProfile {
        &self.profile
    }

    fn status(&self, now_unix: u64) -> AuthStatusBadge {
        if self.tokens.mc_access_token.is_empty() || self.tokens.expires_at_unix <= now_unix + 60 {
            AuthStatusBadge::Expired
        } else if self.tokens.expires_at_unix <= now_unix + 1800 {
            AuthStatusBadge::ExpiringSoon
        } else {
            AuthStatusBadge::Valid
        }
    }
}

/// Local offline profile entity with vanilla v3 deterministic UUID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineAccount {
    pub id: String,
    pub username: String,
    pub uuid_hex: String,
    pub profile: AccountProfile,
}

impl OfflineAccount {
    /// Builds a validated offline account with explicit deterministic UUID.
    pub fn new(id: String, username: String, uuid: u128, now_unix: u64) -> Result<Self, SwitcherError> {
        validate_minecraft_name(&username)?;
        Ok(Self {
            id,
            username,
            uuid_hex: format_uuid_undashed(uuid),
            profile: AccountProfile::with_created(now_unix),
        })
    }
}

impl AccountIdentity for OfflineAccount {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> AccountKind {
        AccountKind::Offline
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn uuid(&self) -> u128 {
        parse_uuid_any(&self.uuid_hex).unwrap_or(0)
    }

    fn profile(&self) -> &AccountProfile {
        &self.profile
    }

    fn status(&self, _now_unix: u64) -> AuthStatusBadge {
        AuthStatusBadge::Offline
    }
}

/// Unified persistent account variant stored inside the vault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "snake_case")]
pub enum AccountRecord {
    Microsoft(MicrosoftAccount),
    Offline(OfflineAccount),
}

impl AccountRecord {
    /// Mutable access to shared profile metadata.
    pub fn profile_mut(&mut self) -> &mut AccountProfile {
        match self {
            Self::Microsoft(acc) => &mut acc.profile,
            Self::Offline(acc) => &mut acc.profile,
        }
    }

    /// Marks the account as recently selected.
    pub fn touch_last_used(&mut self, now_unix: u64) {
        self.profile_mut().last_used_unix = now_unix;
    }

    /// Toggles pinned favorite status.
    pub fn toggle_favorite(&mut self) {
        let profile = self.profile_mut();
        profile.favorite = !profile.favorite;
    }

    /// Borrows the inner Microsoft account when applicable.
    pub fn as_microsoft(&self) -> Option<&MicrosoftAccount> {
        match self {
            Self::Microsoft(acc) => Some(acc),
            Self::Offline(_) => None,
        }
    }

    /// Mutably borrows the inner Microsoft account when applicable.
    pub fn as_microsoft_mut(&mut self) -> Option<&mut MicrosoftAccount> {
        match self {
            Self::Microsoft(acc) => Some(acc),
            Self::Offline(_) => None,
        }
    }
}

impl AccountIdentity for AccountRecord {
    fn id(&self) -> &str {
        match self {
            Self::Microsoft(acc) => acc.id(),
            Self::Offline(acc) => acc.id(),
        }
    }

    fn kind(&self) -> AccountKind {
        match self {
            Self::Microsoft(acc) => acc.kind(),
            Self::Offline(acc) => acc.kind(),
        }
    }

    fn username(&self) -> &str {
        match self {
            Self::Microsoft(acc) => acc.username(),
            Self::Offline(acc) => acc.username(),
        }
    }

    fn uuid(&self) -> u128 {
        match self {
            Self::Microsoft(acc) => acc.uuid(),
            Self::Offline(acc) => acc.uuid(),
        }
    }

    fn profile(&self) -> &AccountProfile {
        match self {
            Self::Microsoft(acc) => acc.profile(),
            Self::Offline(acc) => acc.profile(),
        }
    }

    fn status(&self, now_unix: u64) -> AuthStatusBadge {
        match self {
            Self::Microsoft(acc) => acc.status(now_unix),
            Self::Offline(acc) => acc.status(now_unix),
        }
    }
}

/// Validates vanilla Minecraft username constraints (`1..=16`, `[A-Za-z0-9_]`).
pub fn validate_minecraft_name(name: &str) -> Result<(), SwitcherError> {
    if name.is_empty() || name.len() > 16 {
        return Err(SwitcherError::Validation(
            "Player name must be 1 to 16 ASCII characters".into(),
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(SwitcherError::Validation(
            "Player name may only contain letters, digits, and '_'".into(),
        ));
    }
    Ok(())
}

/// Parses dashed or compact 32-char hexadecimal UUID strings into `u128`.
pub fn parse_uuid_any(text: &str) -> Option<u128> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

/// Formats a `u128` UUID as lowercase 32-char compact hex.
pub fn format_uuid_undashed(uuid: u128) -> String {
    format!("{uuid:032x}")
}

/// Formats a `u128` UUID in canonical `8-4-4-4-12` hyphenated form.
pub fn format_uuid_dashed(uuid: u128) -> String {
    let h = format_uuid_undashed(uuid);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Current UNIX timestamp in whole seconds.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_minecraft_usernames() {
        assert!(validate_minecraft_name("Steve").is_ok());
        assert!(validate_minecraft_name("Player_123456789").is_ok());
        assert!(validate_minecraft_name("").is_err());
        assert!(validate_minecraft_name("TooLongPlayerName").is_err());
        assert!(validate_minecraft_name("Bad Name!").is_err());
    }

    #[test]
    fn formats_and_parses_uuids() {
        let raw = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
        let id = parse_uuid_any(raw).expect("valid uuid");
        assert_eq!(format_uuid_dashed(id), raw);
        assert_eq!(format_uuid_undashed(id), "069a79f444e94726a5befca90e38aaf5");
    }

    #[test]
    fn polymorphic_status_and_expiration_work() {
        let msa = AccountRecord::Microsoft(
            MicrosoftAccount::new(
                "ms-1".into(),
                "CosmoJar".into(),
                12345,
                MicrosoftTokens {
                    mc_access_token: "tok".into(),
                    ms_refresh_token: "ref".into(),
                    expires_at_unix: 1000,
                    xuid: Some("999".into()),
                    ownership_verified: true,
                },
                500,
            )
            .unwrap(),
        );
        assert_eq!(msa.status(500), AuthStatusBadge::ExpiringSoon);
        assert_eq!(msa.status(950), AuthStatusBadge::Expired);
        assert!(msa.is_online());

        let offline = AccountRecord::Offline(
            OfflineAccount::new("off-1".into(), "OfflineDev".into(), 67890, 500).unwrap(),
        );
        assert_eq!(offline.status(99999), AuthStatusBadge::Offline);
        assert!(!offline.is_online());
    }
}
