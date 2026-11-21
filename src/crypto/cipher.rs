// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::domain::SwitcherError;

use super::zeroize_buf::{SecretBytes, wipe_bytes};

type HmacSha256 = Hmac<Sha256>;

/// Default PBKDF2-HMAC-SHA256 work factor for master passwords.
pub const DEFAULT_KDF_ITERATIONS: u32 = 120_000;
const AEAD_INFO: &[u8] = b"horizon-account-switcher-chacha20poly1305-v1";

/// Storage protection level configured by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionMode {
    DeviceKey,
    PasswordProtected,
    Plaintext,
}

/// Authenticated ciphertext bundle encoded for vault serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBlob {
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

/// Symmetric authenticated encryption abstraction for vault payloads.
pub trait VaultCipher {
    fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<EncryptedBlob, SwitcherError>;
    fn decrypt(&self, blob: &EncryptedBlob, aad: &[u8]) -> Result<SecretBytes, SwitcherError>;
}

/// ChaCha20-Poly1305 AEAD cipher keyed via HKDF-SHA256.
pub struct ChaChaVaultCipher {
    key_bytes: [u8; 32],
}

impl ChaChaVaultCipher {
    /// Derives a domain-separated AEAD key from an arbitrary secret (device key or PBKDF2 output).
    pub fn from_ikm(ikm: &[u8], salt: &[u8]) -> Result<Self, SwitcherError> {
        let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
        let mut key_bytes = [0u8; 32];
        hk.expand(AEAD_INFO, &mut key_bytes)
            .map_err(|_| SwitcherError::Crypto("HKDF expansion failed".into()))?;
        Ok(Self { key_bytes })
    }

    /// Derives a cipher from a user master password using PBKDF2-HMAC-SHA256 + HKDF.
    pub fn from_password(
        password: &str,
        salt: &[u8],
        iterations: u32,
    ) -> Result<Self, SwitcherError> {
        let mut pbkdf_key = [0u8; 32];
        derive_key_pbkdf2(password.as_bytes(), salt, iterations.max(10_000), &mut pbkdf_key)?;
        let res = Self::from_ikm(&pbkdf_key, salt);
        wipe_bytes(&mut pbkdf_key);
        res
    }
}

impl Drop for ChaChaVaultCipher {
    fn drop(&mut self) {
        wipe_bytes(&mut self.key_bytes);
    }
}

impl VaultCipher for ChaChaVaultCipher {
    fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<EncryptedBlob, SwitcherError> {
        let mut nonce_raw = [0u8; 12];
        getrandom::fill(&mut nonce_raw)
            .map_err(|e| SwitcherError::Crypto(format!("CSPRNG failed: {e}")))?;

        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key_bytes));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_raw),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| SwitcherError::Crypto("Encryption failure".into()))?;

        Ok(EncryptedBlob {
            nonce_b64: B64.encode(nonce_raw),
            ciphertext_b64: B64.encode(ciphertext),
        })
    }

    fn decrypt(&self, blob: &EncryptedBlob, aad: &[u8]) -> Result<SecretBytes, SwitcherError> {
        let nonce_raw = B64
            .decode(&blob.nonce_b64)
            .map_err(|_| SwitcherError::Crypto("Invalid nonce encoding".into()))?;
        if nonce_raw.len() != 12 {
            return Err(SwitcherError::Crypto("Invalid AEAD nonce length".into()));
        }
        let ciphertext = B64
            .decode(&blob.ciphertext_b64)
            .map_err(|_| SwitcherError::Crypto("Invalid ciphertext encoding".into()))?;

        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key_bytes));
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce_raw),
                Payload {
                    msg: &ciphertext,
                    aad,
                },
            )
            .map_err(|_| {
                SwitcherError::Crypto("Decryption failed (wrong password or tampered vault)".into())
            })?;

        Ok(SecretBytes::new(plaintext))
    }
}

/// RFC 8018 PBKDF2-HMAC-SHA256 key derivation.
pub fn derive_key_pbkdf2(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    out: &mut [u8],
) -> Result<(), SwitcherError> {
    if iterations == 0 || out.is_empty() {
        return Err(SwitcherError::Crypto("Invalid PBKDF2 parameters".into()));
    }
    let mut block_idx: u32 = 1;
    let mut offset = 0usize;

    while offset < out.len() {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(password)
            .map_err(|_| SwitcherError::Crypto("HMAC key setup failed".into()))?;
        mac.update(salt);
        mac.update(&block_idx.to_be_bytes());
        let mut u: [u8; 32] = mac.finalize().into_bytes().into();
        let mut acc = u;

        for _ in 1..iterations {
            let mut step = <HmacSha256 as Mac>::new_from_slice(password)
                .map_err(|_| SwitcherError::Crypto("HMAC iteration failed".into()))?;
            step.update(&u);
            u = step.finalize().into_bytes().into();
            for (a, x) in acc.iter_mut().zip(u.iter()) {
                *a ^= *x;
            }
        }

        let take = (out.len() - offset).min(32);
        out[offset..offset + take].copy_from_slice(&acc[..take]);
        wipe_bytes(&mut u);
        wipe_bytes(&mut acc);
        offset += take;
        block_idx = block_idx
            .checked_add(1)
            .ok_or_else(|| SwitcherError::Crypto("PBKDF2 output too large".into()))?;
    }
    Ok(())
}

/// Generates `len` cryptographically secure random bytes.
pub fn random_bytes(len: usize) -> Result<Vec<u8>, SwitcherError> {
    let mut out = vec![0u8; len];
    getrandom::fill(&mut out)
        .map_err(|e| SwitcherError::Crypto(format!("CSPRNG failed: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_hmac_sha256_matches_rfc_vectors() {
        // Test vector from draft-josefsson-pbkdf2-test-vectors-06 (HMAC-SHA256)
        let mut out = [0u8; 32];
        derive_key_pbkdf2(b"password", b"salt", 2, &mut out).unwrap();
        let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn roundtrip_and_tamper_detection() {
        let salt = b"0123456789abcdef";
        let cipher = ChaChaVaultCipher::from_password("hunter2", salt, 10_000).unwrap();
        let aad = b"account-switcher-test";
        let blob = cipher.encrypt(b"{\"secret\":true}", aad).unwrap();

        let decrypted = cipher.decrypt(&blob, aad).unwrap();
        assert_eq!(decrypted.as_slice(), b"{\"secret\":true}");

        let wrong_pw = ChaChaVaultCipher::from_password("wrong", salt, 10_000).unwrap();
        assert!(wrong_pw.decrypt(&blob, aad).is_err());

        assert!(cipher.decrypt(&blob, b"wrong-aad").is_err());
    }
}
