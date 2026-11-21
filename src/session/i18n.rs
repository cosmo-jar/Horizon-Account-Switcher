// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

/// Supported UI languages synchronized automatically with Horizon's active language setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    EnUs,
    RuRu,
}

impl Locale {
    /// Maps a Horizon language code (such as `"ru_ru"`, `"en_us"`) to a localized pack.
    pub fn from_game_code(code: &str) -> Self {
        let lower = code.trim().to_ascii_lowercase();
        if lower.starts_with("ru") || lower.starts_with("uk") || lower.starts_with("be") {
            Self::RuRu
        } else {
            Self::EnUs
        }
    }
}

/// Complete string table eliminating hardcoded language strings in UI widgets.
pub struct LanguagePack {
    pub locale: Locale,
}

impl LanguagePack {
    pub fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn text(&self, key: &str) -> &'static str {
        match (self.locale, key) {
            (Locale::RuRu, "title") => "Переключатель аккаунтов",
            (_, "title") => "Account Switcher",

            (Locale::RuRu, "subtitle") => "Мгновенное переключение профилей без перезапуска",
            (_, "subtitle") => "Switch Microsoft & offline profiles without restarting",

            (Locale::RuRu, "pill.accounts") => "Аккаунты",
            (_, "pill.accounts") => "Accounts",

            (Locale::RuRu, "launcher.tag") => "Сессия лаунчера",
            (_, "launcher.tag") => "Launcher Session",

            (Locale::RuRu, "mode.online") => "Лицензия MS",
            (_, "mode.online") => "Microsoft",

            (Locale::RuRu, "mode.offline") => "Оффлайн",
            (_, "mode.offline") => "Offline",

            (Locale::RuRu, "status.valid") => "Активен",
            (_, "status.valid") => "Valid",

            (Locale::RuRu, "status.expiring") => "Скоро истечет",
            (_, "status.expiring") => "Expiring Soon",

            (Locale::RuRu, "status.expired") => "Истек (обновл.)",
            (_, "status.expired") => "Expired",

            (Locale::RuRu, "status.offline") => "Локальный",
            (_, "status.offline") => "Offline",

            (Locale::RuRu, "filter.all") => "Все",
            (_, "filter.all") => "All",

            (Locale::RuRu, "search.placeholder") => "Поиск по нику...",
            (_, "search.placeholder") => "Search username...",

            (Locale::RuRu, "btn.add_microsoft") => "+ Microsoft",
            (_, "btn.add_microsoft") => "+ Microsoft",

            (Locale::RuRu, "btn.add_offline") => "+ Оффлайн",
            (_, "btn.add_offline") => "+ Offline",

            (Locale::RuRu, "btn.settings") => "Хранилище",
            (_, "btn.settings") => "Vault",

            (Locale::RuRu, "btn.close") => "Закрыть",
            (_, "btn.close") => "Close",

            (Locale::RuRu, "btn.switch") => "Выбрать",
            (_, "btn.switch") => "Switch",

            (Locale::RuRu, "btn.active") => "Выбран",
            (_, "btn.active") => "Active",

            (Locale::RuRu, "btn.refresh") => "Обновить",
            (_, "btn.refresh") => "Refresh",

            (Locale::RuRu, "btn.delete") => "Удалить",
            (_, "btn.delete") => "Delete",

            (Locale::RuRu, "btn.cancel") => "Отмена",
            (_, "btn.cancel") => "Cancel",

            (Locale::RuRu, "btn.confirm") => "Подтвердить",
            (_, "btn.confirm") => "Confirm",

            (Locale::RuRu, "modal.offline.title") => "Добавить оффлайн-профиль",
            (_, "modal.offline.title") => "Add Offline Profile",

            (Locale::RuRu, "modal.offline.hint") => "Имя игрока (1-16 симв., A-Z, 0-9, _)",
            (_, "modal.offline.hint") => "Player username (1-16 chars, A-Z, 0-9, _)",

            (Locale::RuRu, "modal.ms.title") => "Вход через Microsoft",
            (_, "modal.ms.title") => "Microsoft Sign-In",

            (Locale::RuRu, "modal.ms.browser") => "Через браузер (PKCE)",
            (_, "modal.ms.browser") => "Browser Sign-In (PKCE)",

            (Locale::RuRu, "modal.ms.device") => "Код устройства",
            (_, "modal.ms.device") => "Device Code",

            (Locale::RuRu, "modal.ms.client_id") => "Azure Client ID (или HORIZON_MSA_CLIENT_ID)",
            (_, "modal.ms.client_id") => "Azure Client ID (or HORIZON_MSA_CLIENT_ID)",

            (Locale::RuRu, "modal.ms.start") => "Начать вход",
            (_, "modal.ms.start") => "Start Sign-In",

            (Locale::RuRu, "modal.ms.copy_code") => "Код:",
            (_, "modal.ms.copy_code") => "Code:",

            (Locale::RuRu, "modal.ms.copy_btn") => "Скопировать код",
            (_, "modal.ms.copy_btn") => "Copy Code",

            (Locale::RuRu, "modal.ms.copied") => "Код скопирован в буфер!",
            (_, "modal.ms.copied") => "Code copied to clipboard!",

            (Locale::RuRu, "modal.ms.device_hint") => "Нажмите «Начать вход»: откроется сайт microsoft.com/link и появится одноразовый код (без настройки Client ID).",
            (_, "modal.ms.device_hint") => "Click Start Sign-In to open microsoft.com/link with a one-time code (works without custom Client ID).",

            (Locale::RuRu, "modal.ms.browser_hint") => "Для PKCE укажите свой Azure Client ID ниже (или выберите «Код устройства»).",
            (_, "modal.ms.browser_hint") => "Enter your Azure Client ID below for Browser PKCE (or switch to Device Code).",

            (Locale::RuRu, "modal.vault.title") => "Безопасность и настройки",
            (_, "modal.vault.title") => "Vault Security & Preferences",

            (Locale::RuRu, "modal.vault.device_key") => "Ключ устройства (AEAD)",
            (_, "modal.vault.device_key") => "Device Key (ChaCha20)",

            (Locale::RuRu, "modal.vault.password") => "Мастер-пароль (PBKDF2)",
            (_, "modal.vault.password") => "Master Password (PBKDF2)",

            (Locale::RuRu, "modal.vault.plaintext") => "Без шифрования",
            (_, "modal.vault.plaintext") => "Plaintext JSON",

            (Locale::RuRu, "modal.vault.pass_hint") => "Пароль шифрования (если нужен)",
            (_, "modal.vault.pass_hint") => "Master password (when selected)",

            (Locale::RuRu, "modal.unlock.title") => "Хранилище зашифровано",
            (_, "modal.unlock.title") => "Unlock Encrypted Vault",

            (Locale::RuRu, "modal.unlock.hint") => "Введите мастер-пароль",
            (_, "modal.unlock.hint") => "Enter master password",

            (Locale::RuRu, "empty.accounts") => "Нет сохранённых аккаунтов. Добавьте профиль выше.",
            (_, "empty.accounts") => "No accounts saved yet. Add a Microsoft or offline profile above.",

            (Locale::RuRu, "shortcut.hint") => "Alt+A — открыть/скрыть   •   Esc — закрыть",
            (_, "shortcut.hint") => "Alt+A toggle   •   Esc close",

            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_game_language_codes() {
        assert_eq!(Locale::from_game_code("ru_ru"), Locale::RuRu);
        assert_eq!(Locale::from_game_code("en_us"), Locale::EnUs);
        assert_eq!(
            LanguagePack::new(Locale::RuRu).text("btn.add_offline"),
            "+ Оффлайн"
        );
        assert_eq!(
            LanguagePack::new(Locale::EnUs).text("btn.add_offline"),
            "+ Offline"
        );
    }
}
