// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

pub mod cipher;
pub mod zeroize_buf;

pub use cipher::{ChaChaVaultCipher, EncryptedBlob, EncryptionMode, VaultCipher, derive_key_pbkdf2};
pub use zeroize_buf::SecretBytes;
