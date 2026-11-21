// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

pub mod account;
pub mod error;

pub use account::{
    AccountIdentity, AccountKind, AccountProfile, AccountRecord, AuthStatusBadge, MicrosoftAccount,
    MicrosoftTokens, OfflineAccount, format_uuid_dashed, format_uuid_undashed, parse_uuid_any,
    validate_minecraft_name,
};
pub use error::SwitcherError;
