// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use crate::domain::{AccountRecord, OfflineAccount, SwitcherError, account::unix_now, validate_minecraft_name};

use super::md5::md5_digest;

/// Computes vanilla Minecraft's deterministic UUIDv3 for `"OfflinePlayer:<name>"`.
pub fn offline_uuid_for(username: &str) -> u128 {
    let input = format!("OfflinePlayer:{username}");
    let mut hash = md5_digest(input.as_bytes());
    hash[6] = (hash[6] & 0x0f) | 0x30; // UUID version 3
    hash[8] = (hash[8] & 0x3f) | 0x80; // RFC 4122 variant
    u128::from_be_bytes(hash)
}

/// Offline account factory enforcing Minecraft naming constraints and UUIDv3 generation.
pub struct OfflineAuthProvider;

impl OfflineAuthProvider {
    /// Validates `username` and constructs a ready offline account record.
    pub fn create_account(id: String, username: &str) -> Result<AccountRecord, SwitcherError> {
        let trimmed = username.trim();
        validate_minecraft_name(trimmed)?;
        let uuid = offline_uuid_for(trimmed);
        let account = OfflineAccount::new(id, trimmed.to_owned(), uuid, unix_now())?;
        Ok(AccountRecord::Offline(account))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::format_uuid_dashed;

    #[test]
    fn computes_vanilla_offline_uuid() {
        // Matches vanilla Java `UUID.nameUUIDFromBytes("OfflinePlayer:Steve")` & horizon_protocol::crypto::offline_uuid
        let uuid = offline_uuid_for("Steve");
        assert_eq!(
            format_uuid_dashed(uuid),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
    }

    #[test]
    fn creates_and_validates_offline_account() {
        assert!(OfflineAuthProvider::create_account("o1".into(), "Valid_User").is_ok());
        assert!(OfflineAuthProvider::create_account("o2".into(), "Invalid Name").is_err());
    }
}
