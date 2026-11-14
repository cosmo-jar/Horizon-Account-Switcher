// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    crypto::{
        ChaChaVaultCipher, EncryptedBlob, EncryptionMode, VaultCipher,
        cipher::{DEFAULT_KDF_ITERATIONS, random_bytes},
        zeroize_buf::wipe_bytes,
    },
    domain::{AccountIdentity, AccountRecord, SwitcherError},
};

const VAULT_SCHEMA_VERSION: u32 = 1;
const VAULT_AAD: &[u8] = b"horizon-account-switcher-vault-v1";

/// User-configurable mod preferences persisted alongside the vault header.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModSettings {
    pub custom_client_id: String,
    pub auto_refresh_tokens: bool,
    pub show_title_badge: bool,
    pub show_multiplayer_badge: bool,
    pub glass_opacity: f32,
    pub accent_rgb: u32,
}

impl Default for ModSettings {
    fn default() -> Self {
        Self {
            custom_client_id: std::env::var("HORIZON_MSA_CLIENT_ID").unwrap_or_default(),
            auto_refresh_tokens: true,
            show_title_badge: true,
            show_multiplayer_badge: true,
            glass_opacity: 0.84,
            accent_rgb: 0x6B4BFF,
        }
    }
}

/// Decrypted in-memory state of all saved accounts and settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VaultState {
    pub accounts: Vec<AccountRecord>,
    pub active_account_id: Option<String>,
    pub settings: ModSettings,
}

impl VaultState {
    /// Returns the account matching `id` if present.
    pub fn find(&self, id: &str) -> Option<&AccountRecord> {
        self.accounts.iter().find(|a| a.id() == id)
    }

    /// Mutably returns the account matching `id` if present.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut AccountRecord> {
        self.accounts.iter_mut().find(|a| a.id() == id)
    }
}

/// Result of attempting to load the persisted vault.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadedVault {
    Unlocked {
        state: VaultState,
        mode: EncryptionMode,
    },
    Locked {
        settings: ModSettings,
        mode: EncryptionMode,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VaultFileEnvelope {
    schema: u32,
    mode: EncryptionMode,
    kdf_salt_b64: String,
    kdf_iterations: u32,
    settings: ModSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encrypted: Option<EncryptedBlob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plaintext_payload: Option<VaultSecretPayload>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VaultSecretPayload {
    accounts: Vec<AccountRecord>,
    active_account_id: Option<String>,
}

/// Storage repository contract decoupling the service from concrete persistence.
pub trait AccountRepository {
    fn load(&self, passphrase: Option<&str>) -> Result<LoadedVault, SwitcherError>;
    fn save(
        &self,
        state: &VaultState,
        mode: EncryptionMode,
        passphrase: Option<&str>,
    ) -> Result<(), SwitcherError>;
}

const SECRET_DIR_NAME: &str = "TOKENS_DO_NOT_SEND_TO_ANYONE";
const VAULT_FILE_NAME: &str = "DO_NOT_SEND_TOKENS_vault.json";
const DEVICE_KEY_FILE_NAME: &str = "DO_NOT_SEND_SECRET_KEY_device.key";
const WARNING_README_NAME: &str = "WARNING_TOKENS_DO_NOT_SHARE_WITH_ANYONE.txt";
const WARNING_README_TEXT: &str = "\
================================================================================
SECURITY WARNING / ПРЕДУПРЕЖДЕНИЕ О БЕЗОПАСНОСТИ
================================================================================
This folder stores your encrypted Minecraft/Microsoft account tokens and the
local decryption key inside TOKENS_DO_NOT_SEND_TO_ANYONE/.

NEVER share, copy, archive into modpacks, or send these files to anyone
(including crash reports, support chats, or strangers)! Anyone who receives
BOTH files together can decrypt and access your saved Microsoft/Minecraft tokens.

В этой папке хранятся ваши зашифрованные токены аккаунтов Minecraft/Microsoft
и локальный ключ расшифровки (внутри TOKENS_DO_NOT_SEND_TO_ANYONE/).

НИКОГДА никому не отправляйте и не передавайте эти файлы (в том числе в
баг-репортах, архивах сборок или техподдержке)! Тот, кто получит ОБА файла,
сможет расшифровать ваши сохранённые токены доступа.
================================================================================
";

/// Atomic JSON vault persistence supporting ChaCha20-Poly1305 encryption.
pub struct FileAccountRepository {
    root_dir: PathBuf,
    vault_path: PathBuf,
    device_key_path: PathBuf,
    legacy_vault_path: PathBuf,
    legacy_device_key_path: PathBuf,
}

impl FileAccountRepository {
    /// Stores secret vault and key files inside `dir/TOKENS_DO_NOT_SEND_TO_ANYONE/`.
    pub fn in_dir(dir: &Path) -> Self {
        let secret_dir = dir.join(SECRET_DIR_NAME);
        Self {
            root_dir: dir.to_path_buf(),
            vault_path: secret_dir.join(VAULT_FILE_NAME),
            device_key_path: secret_dir.join(DEVICE_KEY_FILE_NAME),
            legacy_vault_path: dir.join("vault.json"),
            legacy_device_key_path: dir.join("device.key"),
        }
    }

    fn ensure_warning_readme(&self) {
        let _ = fs::create_dir_all(&self.root_dir);
        let root_readme = self.root_dir.join(WARNING_README_NAME);
        if !root_readme.exists() {
            let _ = write_atomic(&root_readme, WARNING_README_TEXT.as_bytes());
        }
        if let Some(secret_dir) = self.vault_path.parent() {
            let _ = fs::create_dir_all(secret_dir);
            let inner_readme = secret_dir.join(WARNING_README_NAME);
            if !inner_readme.exists() {
                let _ = write_atomic(&inner_readme, WARNING_README_TEXT.as_bytes());
            }
        }
    }

    fn migrate_legacy_files(&self) -> Result<(), SwitcherError> {
        let needs_vault = !self.vault_path.is_file() && self.legacy_vault_path.is_file();
        let needs_key = !self.device_key_path.is_file() && self.legacy_device_key_path.is_file();
        if !needs_vault && !needs_key {
            return Ok(());
        }
        if let Some(secret_dir) = self.vault_path.parent() {
            fs::create_dir_all(secret_dir)
                .map_err(|e| SwitcherError::Storage(format!("Create secret dir: {e}")))?;
        }
        if needs_key {
            move_file_atomic(&self.legacy_device_key_path, &self.device_key_path)?;
            restrict_permissions(&self.device_key_path);
        }
        if needs_vault {
            move_file_atomic(&self.legacy_vault_path, &self.vault_path)?;
            restrict_permissions(&self.vault_path);
        }
        self.ensure_warning_readme();
        Ok(())
    }

    fn load_or_create_device_key(&self) -> Result<[u8; 32], SwitcherError> {
        self.migrate_legacy_files()?;
        if self.device_key_path.is_file() {
            let text = fs::read_to_string(&self.device_key_path)
                .map_err(|e| SwitcherError::Storage(format!("Read device key: {e}")))?;
            let bytes = B64
                .decode(text.trim())
                .map_err(|_| SwitcherError::Crypto("Corrupt device key file".into()))?;
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }

        if let Some(parent) = self.device_key_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| SwitcherError::Storage(format!("Create config dir: {e}")))?;
        }
        self.ensure_warning_readme();
        let raw = random_bytes(32)?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&raw);
        let encoded = B64.encode(key);
        write_atomic(&self.device_key_path, encoded.as_bytes())?;
        restrict_permissions(&self.device_key_path);
        Ok(key)
    }

    fn build_cipher(
        &self,
        mode: EncryptionMode,
        salt: &[u8],
        iterations: u32,
        passphrase: Option<&str>,
    ) -> Result<Option<ChaChaVaultCipher>, SwitcherError> {
        match mode {
            EncryptionMode::Plaintext => Ok(None),
            EncryptionMode::DeviceKey => {
                let mut key = self.load_or_create_device_key()?;
                let cipher = ChaChaVaultCipher::from_ikm(&key, salt);
                wipe_bytes(&mut key);
                Ok(Some(cipher?))
            }
            EncryptionMode::PasswordProtected => {
                let pw = passphrase
                    .filter(|p| !p.is_empty())
                    .ok_or(SwitcherError::LockedVault)?;
                Ok(Some(ChaChaVaultCipher::from_password(pw, salt, iterations)?))
            }
        }
    }
}

impl AccountRepository for FileAccountRepository {
    fn load(&self, passphrase: Option<&str>) -> Result<LoadedVault, SwitcherError> {
        self.migrate_legacy_files()?;
        if !self.vault_path.is_file() {
            return Ok(LoadedVault::Unlocked {
                state: VaultState::default(),
                mode: EncryptionMode::DeviceKey,
            });
        }

        let raw = fs::read_to_string(&self.vault_path)
            .map_err(|e| SwitcherError::Storage(format!("Read vault: {e}")))?;
        let env: VaultFileEnvelope = serde_json::from_str(&raw)
            .map_err(|e| SwitcherError::Storage(format!("Parse vault JSON: {e}")))?;
        if env.schema != VAULT_SCHEMA_VERSION {
            return Err(SwitcherError::Storage(format!(
                "Unsupported vault schema v{}",
                env.schema
            )));
        }

        let salt = B64
            .decode(&env.kdf_salt_b64)
            .map_err(|_| SwitcherError::Crypto("Invalid vault salt".into()))?;

        match env.mode {
            EncryptionMode::Plaintext => {
                let payload = env.plaintext_payload.unwrap_or(VaultSecretPayload {
                    accounts: Vec::new(),
                    active_account_id: None,
                });
                Ok(LoadedVault::Unlocked {
                    state: VaultState {
                        accounts: payload.accounts,
                        active_account_id: payload.active_account_id,
                        settings: env.settings,
                    },
                    mode: env.mode,
                })
            }
            EncryptionMode::PasswordProtected if passphrase.is_none() => Ok(LoadedVault::Locked {
                settings: env.settings,
                mode: env.mode,
            }),
            EncryptionMode::DeviceKey | EncryptionMode::PasswordProtected => {
                let cipher = self
                    .build_cipher(env.mode, &salt, env.kdf_iterations, passphrase)?
                    .ok_or_else(|| SwitcherError::Crypto("Missing cipher".into()))?;
                let blob = env
                    .encrypted
                    .as_ref()
                    .ok_or_else(|| SwitcherError::Storage("Missing encrypted vault payload".into()))?;
                let secret = cipher.decrypt(blob, VAULT_AAD)?;
                let payload: VaultSecretPayload = serde_json::from_slice(secret.as_slice())
                    .map_err(|e| SwitcherError::Storage(format!("Corrupt payload JSON: {e}")))?;
                Ok(LoadedVault::Unlocked {
                    state: VaultState {
                        accounts: payload.accounts,
                        active_account_id: payload.active_account_id,
                        settings: env.settings,
                    },
                    mode: env.mode,
                })
            }
        }
    }

    fn save(
        &self,
        state: &VaultState,
        mode: EncryptionMode,
        passphrase: Option<&str>,
    ) -> Result<(), SwitcherError> {
        let dir = self
            .vault_path
            .parent()
            .ok_or_else(|| SwitcherError::Storage("Invalid vault path".into()))?;
        fs::create_dir_all(dir)
            .map_err(|e| SwitcherError::Storage(format!("Create config dir: {e}")))?;
        self.migrate_legacy_files()?;
        self.ensure_warning_readme();

        let salt = random_bytes(16)?;
        let payload = VaultSecretPayload {
            accounts: state.accounts.clone(),
            active_account_id: state.active_account_id.clone(),
        };

        let envelope = match mode {
            EncryptionMode::Plaintext => VaultFileEnvelope {
                schema: VAULT_SCHEMA_VERSION,
                mode,
                kdf_salt_b64: B64.encode(&salt),
                kdf_iterations: DEFAULT_KDF_ITERATIONS,
                settings: state.settings.clone(),
                encrypted: None,
                plaintext_payload: Some(payload),
            },
            EncryptionMode::DeviceKey | EncryptionMode::PasswordProtected => {
                let cipher = self
                    .build_cipher(mode, &salt, DEFAULT_KDF_ITERATIONS, passphrase)?
                    .ok_or_else(|| SwitcherError::Crypto("Missing cipher".into()))?;
                let mut json_bytes = serde_json::to_vec(&payload)
                    .map_err(|e| SwitcherError::Storage(format!("Serialize payload: {e}")))?;
                let blob = cipher.encrypt(&json_bytes, VAULT_AAD);
                wipe_bytes(&mut json_bytes);
                VaultFileEnvelope {
                    schema: VAULT_SCHEMA_VERSION,
                    mode,
                    kdf_salt_b64: B64.encode(&salt),
                    kdf_iterations: DEFAULT_KDF_ITERATIONS,
                    settings: state.settings.clone(),
                    encrypted: Some(blob?),
                    plaintext_payload: None,
                }
            }
        };

        let pretty = serde_json::to_vec_pretty(&envelope)
            .map_err(|e| SwitcherError::Storage(format!("Serialize vault: {e}")))?;
        write_atomic(&self.vault_path, &pretty)?;
        restrict_permissions(&self.vault_path);
        Ok(())
    }
}

fn move_file_atomic(from: &Path, to: &Path) -> Result<(), SwitcherError> {
    if let Err(_) = fs::rename(from, to) {
        let data = fs::read(from)
            .map_err(|e| SwitcherError::Storage(format!("Read legacy {}: {e}", from.display())))?;
        write_atomic(to, &data)?;
        let _ = fs::remove_file(from);
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SwitcherError> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| SwitcherError::Storage(format!("Write {}: {e}", tmp.display())))?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).map_err(|e| SwitcherError::Storage(format!("Rename {}: {e}", path.display())))?;
    Ok(())
}

fn restrict_permissions(_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(_path, fs::Permissions::from_mode(0o600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::OfflineAccount;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon_as_test_{name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn device_key_roundtrip_and_password_lock() {
        let dir = temp_dir("vault");
        let repo = FileAccountRepository::in_dir(&dir);

        let mut state = VaultState::default();
        state.accounts.push(AccountRecord::Offline(
            OfflineAccount::new("id1".into(), "AlexTest".into(), 99, 1000).unwrap(),
        ));
        state.active_account_id = Some("id1".into());

        // Device key roundtrip
        repo.save(&state, EncryptionMode::DeviceKey, None).unwrap();
        assert!(dir.join(SECRET_DIR_NAME).join(VAULT_FILE_NAME).is_file());
        assert!(dir.join(SECRET_DIR_NAME).join(DEVICE_KEY_FILE_NAME).is_file());
        assert!(dir.join(WARNING_README_NAME).is_file());
        assert!(dir.join(SECRET_DIR_NAME).join(WARNING_README_NAME).is_file());

        match repo.load(None).unwrap() {
            LoadedVault::Unlocked { state: loaded, mode } => {
                assert_eq!(mode, EncryptionMode::DeviceKey);
                assert_eq!(loaded.accounts.len(), 1);
                assert_eq!(loaded.active_account_id.as_deref(), Some("id1"));
            }
            _ => panic!("expected unlocked vault"),
        }

        // Password protected lock -> unlock -> wrong password check
        repo.save(&state, EncryptionMode::PasswordProtected, Some("S3cr3t!"))
            .unwrap();
        assert!(matches!(repo.load(None).unwrap(), LoadedVault::Locked { .. }));
        assert!(repo.load(Some("wrong")).is_err());
        match repo.load(Some("S3cr3t!")).unwrap() {
            LoadedVault::Unlocked { state: loaded, mode } => {
                assert_eq!(mode, EncryptionMode::PasswordProtected);
                assert_eq!(loaded.accounts[0].username(), "AlexTest");
            }
            _ => panic!("expected unlocked"),
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_legacy_vault_and_device_key_into_warning_dir() {
        let dir = temp_dir("legacy_migration");
        let legacy_repo = FileAccountRepository::in_dir(&dir);

        let mut state = VaultState::default();
        state.accounts.push(AccountRecord::Offline(
            OfflineAccount::new("id_old".into(), "LegacyUser".into(), 77, 1000).unwrap(),
        ));
        state.active_account_id = Some("id_old".into());
        legacy_repo
            .save(&state, EncryptionMode::DeviceKey, None)
            .unwrap();

        // Simulate legacy files sitting directly in `dir`
        fs::rename(
            dir.join(SECRET_DIR_NAME).join(VAULT_FILE_NAME),
            dir.join("vault.json"),
        )
        .unwrap();
        fs::rename(
            dir.join(SECRET_DIR_NAME).join(DEVICE_KEY_FILE_NAME),
            dir.join("device.key"),
        )
        .unwrap();
        let _ = fs::remove_dir_all(dir.join(SECRET_DIR_NAME));
        let _ = fs::remove_file(dir.join(WARNING_README_NAME));

        let migrated_repo = FileAccountRepository::in_dir(&dir);
        match migrated_repo.load(None).unwrap() {
            LoadedVault::Unlocked { state: loaded, .. } => {
                assert_eq!(loaded.accounts[0].username(), "LegacyUser");
                assert_eq!(loaded.active_account_id.as_deref(), Some("id_old"));
            }
            _ => panic!("expected unlocked migrated vault"),
        }

        assert!(!dir.join("vault.json").exists());
        assert!(!dir.join("device.key").exists());
        assert!(dir.join(SECRET_DIR_NAME).join(VAULT_FILE_NAME).is_file());
        assert!(dir.join(SECRET_DIR_NAME).join(DEVICE_KEY_FILE_NAME).is_file());
        assert!(dir.join(WARNING_README_NAME).is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}
