// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use std::fmt;

/// Typed error hierarchy for account validation, encryption, storage, and OAuth operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitcherError {
    Validation(String),
    Crypto(String),
    Storage(String),
    Network(String),
    OAuth(String),
    LockedVault,
}

impl SwitcherError {
    /// Short machine-friendly category tag for logs and localized error badges.
    pub fn category(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation",
            Self::Crypto(_) => "crypto",
            Self::Storage(_) => "storage",
            Self::Network(_) => "network",
            Self::OAuth(_) => "oauth",
            Self::LockedVault => "locked_vault",
        }
    }
}

impl fmt::Display for SwitcherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(msg)
            | Self::Crypto(msg)
            | Self::Storage(msg)
            | Self::Network(msg)
            | Self::OAuth(msg) => f.write_str(msg),
            Self::LockedVault => f.write_str("Vault is locked; enter your master password"),
        }
    }
}

impl std::error::Error for SwitcherError {}
