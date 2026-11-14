# Horizon Account Switcher

An original, native Rust mod for **Horizon (Minecraft 26.3+)** that lets you add, manage, and switch between Microsoft (online) and offline accounts directly inside the game without restarting.

<img width="592" height="326" alt="Скриншот 14-11-2026 025057" src="https://github.com/user-attachments/assets/d06a2f75-6765-4e06-b152-3f6bb3778562" />

<img width="1621" height="964" alt="Скриншот 14-11-2026 025124" src="https://github.com/user-attachments/assets/99229d6a-6ff8-4c9a-ba8e-dc3d787aa1fd" />

<img width="1919" height="1079" alt="Скриншот 14-11-2026 030824" src="https://github.com/user-attachments/assets/92fbfe00-246d-4efa-9c07-ae4d721d38b3" />


## Features

- **In-Game Account Switching**: Swap sessions live from the **Title Screen**, **Multiplayer Screen**, **Pause Menu**, or **Options → Mods** via the frosted profile pill or `Alt+A`.
- **Launcher Session Rollback**: Keeps a pinned **Launcher Session** card at the top of your list so you can switch back to the original Horizon Launcher session at any time, and automatically rolls back to it when you delete the active or last saved profile.
- **Liquid-Glass UI**: Frosted translucent obsidian-violet panels aligned with Horizon's `Glass` design tokens (`#6B4BFF`, `#4ADE80`, `#F2C46B`, `#FF8A8A`), specular rim highlights, drop shadows, smooth scroll clipping, search/filtering, `Ctrl+V` / clipboard paste, and `8x8` composited skin avatars.
- **Microsoft OAuth 2.0 Authentication**: Works out of the box via Device Code flow or optionally via your own Azure Client ID with Browser PKCE.
- **Offline Account Support**: Username validation (`1–16` ASCII `[A-Za-z0-9_]`) and deterministic vanilla `UUIDv3` (`OfflinePlayer:<name>`) computation.
- **Encrypted Credential Vault**: ChaCha20-Poly1305 AEAD storage (`Device Key` by default, optional `Master Password` via PBKDF2-HMAC-SHA256 `120,000` rounds, or `Plaintext`).
- **Automatic Localization**: Adapts dynamically between English (`en_us`) and Russian (`ru_ru`) based on Horizon's active language setting.

## How Microsoft Authentication Works (No Third-Party Servers)

All authentication traffic goes **directly** from your computer to official Microsoft, Xbox Live, and Mojang/Minecraft servers (`mixins/account_switcher/engine/src/auth/microsoft.rs` and `skins.rs`). There are **no proxy servers, telemetry endpoints, or mod-author backends**:

1. **Microsoft OAuth 2.0** (two modes):
   - **Default (`Device Code`)**: Uses the built-in public Live client ID (`DEFAULT_LIVE_CLIENT_ID = "00000000441cc96b"`) against `https://login.live.com/oauth20_connect.srf` and `https://login.live.com/oauth20_token.srf`. The mod requests a short one-time code, opens Microsoft's login page in your browser, and you sign in directly on Microsoft's site.
   - **Custom Azure Client ID (Optional)**: If configured in **Vault Security & Preferences** or via `HORIZON_MSA_CLIENT_ID`, the mod uses `https://login.microsoftonline.com/consumers/oauth2/v2.0` (supporting both **Device Code** and **Browser Authorization Code + PKCE S256** with a local loopback callback on `http://127.0.0.1:<port>/callback`).
2. **Official Xbox & Minecraft Token Exchange**:
   - `https://user.auth.xboxlive.com/user/authenticate` (Xbox Live)
   - `https://xsts.auth.xboxlive.com/xsts/authorize` (XSTS for `rp://api.minecraftservices.com/`)
   - `https://api.minecraftservices.com/authentication/login_with_xbox` (Minecraft Java access token)
   - `https://api.minecraftservices.com/entitlements/mcstore` (game ownership check)
   - `https://api.minecraftservices.com/minecraft/profile` (UUID, username, skin URL)
   - `https://sessionserver.mojang.com/` & `https://textures.minecraft.net/texture/` (skin head avatars)
3. **Local-Only Session Use**: The resulting `mc_access_token` is used solely in-memory by the Minecraft client when connecting to multiplayer servers.

## Local Credential Storage & Security Warning

All saved profiles and tokens stay **strictly on your local disk** inside Horizon's per-mod data directory:

```text
<horizon_game_dir>/horizon-mods/.horizon-mod-data/account_switcher/
├── WARNING_TOKENS_DO_NOT_SHARE_WITH_ANYONE.txt
└── TOKENS_DO_NOT_SEND_TO_ANYONE/
    ├── WARNING_TOKENS_DO_NOT_SHARE_WITH_ANYONE.txt
    ├── DO_NOT_SEND_TOKENS_vault.json          # Account list & encrypted Microsoft/MC tokens
    └── DO_NOT_SEND_SECRET_KEY_device.key      # 256-bit local key (used in Device Key mode)
```

*(Older `vault.json` and `device.key` files created by earlier versions are automatically migrated into `TOKENS_DO_NOT_SEND_TO_ANYONE/` on first load or save.)*

> [!WARNING]
> **NEVER copy, archive, or send `TOKENS_DO_NOT_SEND_TO_ANYONE/` (or `.horizon-mod-data/account_switcher/`) to anyone** — including in bug reports, crash dumps, modpacks, or support chats!
>
> By default (`Device Key (ChaCha20)`), `DO_NOT_SEND_TOKENS_vault.json` is encrypted automatically in the background with no password prompt using `DO_NOT_SEND_SECRET_KEY_device.key` stored alongside it. Anyone who obtains **both files together** can decrypt your saved refresh/access tokens.
>
> For stronger protection against disk/file theft, open **Vault** in the mod UI and switch to **Master Password (`PBKDF2`)** (`120,000` PBKDF2-HMAC-SHA256 iterations + HKDF-SHA256 + ChaCha20-Poly1305 AEAD). In that mode `device.key` is not used, and the vault cannot be decrypted without your password.

## Compatibility & Pinned Horizon Commit

- **Target Game**: Minecraft `26.3+`
- **Pinned Horizon Commit**: `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` (set in `compatibility.json` and `mixins/account_switcher/horizon-mixin.toml`)
- **Horizon Mixins Revision**: `d9c83d7096b7ea5b83678da2efdfd6161f74997e`

Because Horizon does not yet expose an official hook point in `native_hooks.rs` for overriding the active auth session, this mod injects a `RETURN` mixin into `horizon_client_render::auth::current` alongside Horizon's standard `native_*` hooks (`native_client_tick_pre`, `native_mods`, `native_settings`, `native_settings_click`, `native_screen_*`). Per Horizon's native mod rules, raw function injections pin a tested `horizon_revision`. On newer Nightlies, Horizon Launcher can still rebuild it via **`Rebuild on Horizon`** (`--ignore-pins`) or **`Build anyway`** as long as `horizon_client_render::auth::current` remains compatible.

## Architecture (Modular Rust Crate)

```text
mixins/account_switcher/
├── horizon-mixin.toml           # Mixin targets, crate wiring, compatibility revision
├── auth_override.rs             # In-process AuthSession & player name override module
├── mod_glue.rs                  # Horizon SessionBridge & HudBatch UiCanvas adapters
└── engine/                      # Pure-Rust core crate
    └── src/
        ├── domain/              # AccountIdentity trait, MicrosoftAccount, OfflineAccount, errors
        ├── crypto/              # VaultCipher trait, ChaChaVaultCipher, PBKDF2-HMAC-SHA256, SecretBytes
        ├── storage/             # AccountRepository trait, atomic FileAccountRepository + migration
        ├── auth/                # MicrosoftOAuthProvider, OfflineAuthProvider, MD5, SkinAvatarCache
        ├── session/             # AccountSwitcherService orchestrator & LanguagePack i18n
        └── ui/                  # Widget trait, LiquidGlassTheme, UiCanvas, AccountSwitcherController
```

## Building, Testing & Packaging

```powershell
# 1. Run all unit & UI controller tests
cargo test

# 2. Package deterministic .hnmod into dist/
python build_package.py
```

## License

GNU General Public License v3.0 ([LICENSE](LICENSE))
