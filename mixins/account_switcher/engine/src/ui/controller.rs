// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

use std::sync::Arc;

use crate::{
    auth::OAuthLoginKind,
    crypto::EncryptionMode,
    domain::{AccountIdentity, AuthStatusBadge, account::unix_now},
    session::{
        AccountFilter, AccountSwitcherService, ClipboardBridge, LanguagePack, Locale, NoopClipboard,
        service::AuthJobState,
    },
};

use super::{
    canvas::{
        Rect, UiCanvas, draw_avatar_grid, draw_liquid_glass_panel, draw_rounded_outline,
        draw_rounded_rect, draw_truncated_text, draw_wrapped_text,
    },
    theme::{LiquidGlassTheme, argb, with_alpha},
    widgets::{ButtonTone, CardHitRects, GlassButtonWidget, GlassTextField, Widget},
};

const KEY_ESCAPE: &str = "key.keyboard.escape";
const KEY_ENTER: &str = "key.keyboard.enter";
const KEY_KP_ENTER: &str = "key.keyboard.keypad.enter";
const KEY_BACKSPACE: &str = "key.keyboard.backspace";
const KEY_DELETE: &str = "key.keyboard.delete";
const KEY_A: &str = "key.keyboard.a";
const KEY_V: &str = "key.keyboard.v";
const KEY_SHIFT_MASK: u32 = 1;
const KEY_CTRL_MASK: u32 = 2;
const KEY_ALT_MASK: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
enum ModalView {
    None,
    AddOffline,
    AddMicrosoft { method: OAuthLoginKind },
    VaultSettings {
        selected_mode: EncryptionMode,
        auto_refresh: bool,
        show_title_badge: bool,
        show_mp_badge: bool,
    },
    UnlockVault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveField {
    Search,
    OfflineName,
    MsClientId,
    VaultPassword,
    VaultClientId,
    UnlockPassword,
}

#[derive(Clone, Debug)]
enum ClickTarget {
    OpenOverlay,
    CloseOverlay,
    Filter(AccountFilter),
    FocusField(ActiveField),
    OpenAddMicrosoft,
    OpenAddOffline,
    OpenVaultSettings,
    RestoreLauncherSession,
    SwitchAccount(String),
    ToggleFavorite(String),
    RefreshAccount(String),
    DeleteAccount(String),
    ConfirmAddOffline,
    CloseModal,
    SelectMsMethod(OAuthLoginKind),
    StartMsLogin,
    OpenDeviceLink(String),
    CopyDeviceCode(String),
    SelectEncryptionMode(EncryptionMode),
    ToggleAutoRefresh,
    ToggleTitleBadge,
    ToggleMpBadge,
    SaveVaultSettings,
    SubmitUnlock,
}

/// Interactive MVC controller owning the overlay open state, inputs, modals, and layout geometry.
pub struct AccountSwitcherController {
    service: AccountSwitcherService,
    open: bool,
    filter: AccountFilter,
    scroll_y: f32,
    max_scroll: f32,
    modal: ModalView,
    active_field: ActiveField,
    search_field: GlassTextField,
    offline_name_field: GlassTextField,
    ms_client_id_field: GlassTextField,
    vault_password_field: GlassTextField,
    vault_client_id_field: GlassTextField,
    unlock_password_field: GlassTextField,
    confirm_delete_id: Option<String>,
    last_copied_code: Option<String>,
    clipboard: Arc<dyn ClipboardBridge>,
    last_hit_regions: Vec<(Rect, ClickTarget)>,
}

impl AccountSwitcherController {
    /// Wraps an initialized `AccountSwitcherService` ready for Horizon screen hooks.
    pub fn new(service: AccountSwitcherService) -> Self {
        let initial_cid = service.settings().custom_client_id.clone();
        let mut ms_client_id_field = GlassTextField::new(64, false);
        ms_client_id_field.value = initial_cid.clone();
        let mut vault_client_id_field = GlassTextField::new(64, false);
        vault_client_id_field.value = initial_cid;

        Self {
            service,
            open: false,
            filter: AccountFilter::All,
            scroll_y: 0.0,
            max_scroll: 0.0,
            modal: ModalView::None,
            active_field: ActiveField::Search,
            search_field: GlassTextField::new(32, false),
            offline_name_field: GlassTextField::new(16, false),
            ms_client_id_field,
            vault_password_field: GlassTextField::new(64, true),
            vault_client_id_field,
            unlock_password_field: GlassTextField::new(64, true),
            confirm_delete_id: None,
            last_copied_code: None,
            clipboard: Arc::new(NoopClipboard),
            last_hit_regions: Vec::new(),
        }
    }

    /// Attaches a system clipboard implementation (used in-game via Horizon's `SystemClipboard`).
    pub fn with_clipboard(mut self, clipboard: Arc<dyn ClipboardBridge>) -> Self {
        self.clipboard = clipboard;
        self
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the switcher overlay (or unlock dialog if vault is locked).
    pub fn open_overlay(&mut self) {
        self.open = true;
        self.scroll_y = 0.0;
        self.confirm_delete_id = None;
        if !self.service.is_unlocked() {
            self.modal = ModalView::UnlockVault;
            self.active_field = ActiveField::UnlockPassword;
        } else {
            self.modal = ModalView::None;
            self.active_field = ActiveField::Search;
        }
    }

    pub fn close_overlay(&mut self) {
        self.open = false;
        self.modal = ModalView::None;
        self.confirm_delete_id = None;
        self.service.cancel_auth_job();
    }

    /// Returns whether the top-right badge should draw on screen `screen_id`.
    pub fn should_draw_badge_on(&self, screen_id: &str) -> bool {
        let s = self.service.settings();
        match screen_id {
            "title" | "world_select" => s.show_title_badge,
            "multiplayer" => s.show_multiplayer_badge,
            _ => false,
        }
    }

    /// Renders badge and/or full modal overlay onto `canvas` and updates interactive hitboxes.
    pub fn render(
        &mut self,
        canvas: &mut dyn UiCanvas,
        screen_id: &str,
        mouse: [f32; 2],
        lang_code: &str,
    ) {
        self.service.poll_background();
        if let AuthJobState::Running {
            device_code: Some(dc),
            ..
        } = &self.service.auth_job
            && self.last_copied_code.as_deref() != Some(dc.user_code.as_str())
        {
            self.clipboard.set_text(&dc.user_code);
            self.last_copied_code = Some(dc.user_code.clone());
        }
        if let AuthJobState::Succeeded(name) = &self.service.auth_job
            && matches!(self.modal, ModalView::AddMicrosoft { .. })
        {
            let msg = format!("{}: {name}", lang_label(lang_code, "Вход выполнен", "Signed in"));
            self.service.banner_message = Some((msg, false));
            self.service.auth_job = AuthJobState::Idle;
            self.last_copied_code = None;
            self.modal = ModalView::None;
            self.active_field = ActiveField::Search;
        }
        self.last_hit_regions.clear();

        let locale = Locale::from_game_code(lang_code);
        let lang = LanguagePack::new(locale);
        let settings = self.service.settings().clone();
        let theme = LiquidGlassTheme::new(settings.accent_rgb, settings.glass_opacity);

        if !self.open {
            if self.should_draw_badge_on(screen_id) {
                self.render_trigger_pill(canvas, &theme, &lang, mouse);
            }
            return;
        }

        // Full-screen translucent liquid-glass backdrop scrim
        let (sw, sh) = canvas.screen_size();
        canvas.fill_rect(Rect::new(0.0, 0.0, sw, sh), theme.scrim());

        let panel_w = (sw - 40.0).clamp(320.0, 520.0);
        let panel_h = (sh - 34.0).clamp(240.0, 390.0);
        let panel = Rect::new(
            ((sw - panel_w) * 0.5).floor(),
            ((sh - panel_h) * 0.5).floor(),
            panel_w,
            panel_h,
        );

        draw_liquid_glass_panel(canvas, &theme, panel, 16.0);
        self.render_main_panel(canvas, &theme, &lang, panel, mouse);

        match self.modal.clone() {
            ModalView::None => {}
            ModalView::AddOffline => {
                self.render_add_offline_modal(canvas, &theme, &lang, panel, mouse)
            }
            ModalView::AddMicrosoft { method } => {
                self.render_add_microsoft_modal(canvas, &theme, &lang, panel, method, mouse)
            }
            ModalView::VaultSettings {
                selected_mode,
                auto_refresh,
                show_title_badge,
                show_mp_badge,
            } => self.render_vault_settings_modal(
                canvas,
                &theme,
                &lang,
                panel,
                selected_mode,
                auto_refresh,
                show_title_badge,
                show_mp_badge,
                mouse,
            ),
            ModalView::UnlockVault => {
                self.render_unlock_modal(canvas, &theme, &lang, panel, mouse)
            }
        }
    }

    fn render_trigger_pill(
        &mut self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        lang: &LanguagePack,
        mouse: [f32; 2],
    ) {
        let (sw, _) = canvas.screen_size();
        let (username, is_online) = self.service.live_identity();
        let label = format!("{}  •  {}", username, lang.text("pill.accounts"));
        let tw = canvas.text_width(&label);
        let pill_w = (tw + 38.0).clamp(112.0, 248.0);
        let pill = Rect::new(sw - pill_w - 8.0, 6.0, pill_w, 22.0);
        let hovered = pill.contains(mouse[0], mouse[1]);

        draw_rounded_rect(
            canvas,
            Rect::new(pill.x - 1.0, pill.y + 2.0, pill.w + 2.0, pill.h + 2.0),
            11.0,
            argb(5, 4, 15, 68),
        );
        draw_rounded_rect(
            canvas,
            pill,
            11.0,
            if hovered {
                argb(42, 36, 76, 225)
            } else {
                argb(20, 18, 42, 188)
            },
        );
        draw_rounded_outline(
            canvas,
            pill,
            11.0,
            1.0,
            if hovered {
                theme.border_highlight()
            } else {
                theme.border_subtle()
            },
            theme.border_subtle(),
        );

        let status_col = if is_online {
            theme.live_emerald()
        } else {
            theme.warn_amber()
        };
        canvas.fill_rect(
            Rect::new(pill.x + 9.0, pill.y + 8.0, 6.0, 6.0),
            status_col,
        );
        canvas.draw_text(
            &label,
            pill.x + 21.0,
            pill.y + 6.5,
            theme.ink_primary(),
            true,
        );
        self.last_hit_regions.push((pill, ClickTarget::OpenOverlay));
    }

    fn render_main_panel(
        &mut self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        lang: &LanguagePack,
        panel: Rect,
        mouse: [f32; 2],
    ) {
        let pad = 14.0;
        let inner_w = panel.w - pad * 2.0;

        // Header strip
        canvas.draw_text(
            lang.text("title"),
            panel.x + pad,
            panel.y + 12.0,
            theme.ink_primary(),
            true,
        );
        let (active_name, active_online) = self.service.live_identity();
        let identity_tag = format!(
            "{} ({})",
            active_name,
            if active_online {
                lang.text("mode.online")
            } else {
                lang.text("mode.offline")
            }
        );
        canvas.draw_text(
            &identity_tag,
            panel.x + pad,
            panel.y + 24.0,
            if active_online {
                theme.live_emerald()
            } else {
                theme.warn_amber()
            },
            false,
        );

        let close_btn = GlassButtonWidget::new(
            Rect::new(panel.x + panel.w - pad - 24.0, panel.y + 11.0, 24.0, 20.0),
            "X",
            ButtonTone::Danger,
        );
        close_btn.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((close_btn.rect, ClickTarget::CloseOverlay));

        let vault_btn = GlassButtonWidget::new(
            Rect::new(close_btn.rect.x - 80.0, panel.y + 11.0, 74.0, 20.0),
            lang.text("btn.settings"),
            ButtonTone::Secondary,
        );
        vault_btn.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((vault_btn.rect, ClickTarget::OpenVaultSettings));

        // Toolbar row: search + filter pills
        let tool_y = panel.y + 42.0;
        let search_w = (inner_w - 178.0).max(110.0);
        let search_rect = Rect::new(panel.x + pad, tool_y, search_w, 22.0);
        self.search_field.focused = self.modal == ModalView::None && self.active_field == ActiveField::Search;
        self.search_field
            .render_in(canvas, theme, search_rect, lang.text("search.placeholder"));
        self.last_hit_regions
            .push((search_rect, ClickTarget::FocusField(ActiveField::Search)));

        let filters = [
            (AccountFilter::All, lang.text("filter.all"), 46.0),
            (AccountFilter::Microsoft, "MS", 48.0),
            (AccountFilter::Offline, lang.text("mode.offline"), 72.0),
        ];
        let mut fx = search_rect.x + search_rect.w + 6.0;
        for (f_kind, f_label, fw) in filters {
            let btn = GlassButtonWidget::new(
                Rect::new(fx, tool_y, fw, 22.0),
                f_label,
                if self.filter == f_kind {
                    ButtonTone::Primary
                } else {
                    ButtonTone::Secondary
                },
            );
            btn.draw(canvas, theme, mouse);
            self.last_hit_regions
                .push((btn.rect, ClickTarget::Filter(f_kind)));
            fx += fw + 4.0;
        }

        // Scrollable account cards viewport
        let list_y = tool_y + 29.0;
        let footer_h = 46.0;
        let list_h = (panel.y + panel.h - footer_h - list_y - 6.0).max(80.0);
        let list_rect = Rect::new(panel.x + pad, list_y, inner_w, list_h);
        draw_rounded_rect(canvas, list_rect, 10.0, argb(6, 5, 14, 118));
        draw_rounded_outline(
            canvas,
            list_rect,
            10.0,
            1.0,
            argb(255, 255, 255, 28),
            argb(255, 255, 255, 14),
        );

        let rows_snapshot: Vec<_> = self
            .service
            .visible_accounts(&self.search_field.value, self.filter)
            .into_iter()
            .cloned()
            .collect();
        let needle = self.search_field.value.trim().to_ascii_lowercase();
        let launcher_card = self.service.launcher_session().filter(|info| {
            let filter_ok = match self.filter {
                AccountFilter::All => true,
                AccountFilter::Microsoft => info.is_online,
                AccountFilter::Offline => !info.is_online,
            };
            let search_ok = needle.is_empty() || info.username.to_ascii_lowercase().contains(&needle);
            filter_ok && search_ok
        });

        let row_h = 44.0;
        let row_gap = 6.0;
        let card_offset = usize::from(launcher_card.is_some());
        let total_rows = card_offset + rows_snapshot.len();
        let total_h = total_rows as f32 * (row_h + row_gap) + 8.0;
        self.max_scroll = (total_h - list_h).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll);

        let mut hit_cards: Vec<CardHitRects> = Vec::new();
        let mut launcher_hit_rect: Option<Rect> = None;
        let clip = canvas.begin_clip();

        if let Some(info) = launcher_card.as_ref() {
            let ry = list_rect.y + 6.0 - self.scroll_y;
            if ry + row_h >= list_rect.y && ry <= list_rect.y + list_h {
                let row_rect = Rect::new(list_rect.x + 6.0, ry, list_rect.w - 12.0, row_h);
                let is_active = self.service.active_account_id().is_none();
                let hovered = self.modal == ModalView::None
                    && list_rect.contains(mouse[0], mouse[1])
                    && row_rect.contains(mouse[0], mouse[1]);

                draw_rounded_rect(canvas, row_rect, 9.0, theme.card_fill(hovered, is_active));
                draw_rounded_outline(
                    canvas,
                    row_rect,
                    9.0,
                    1.0,
                    if is_active {
                        theme.live_emerald()
                    } else if hovered {
                        theme.border_highlight()
                    } else {
                        theme.border_subtle()
                    },
                    with_alpha(theme.border_subtle(), 70),
                );

                let grid = self.service.launcher_avatar(info);
                let avatar_rect = Rect::new(row_rect.x + 8.0, row_rect.y + 6.0, 32.0, 32.0);
                draw_avatar_grid(
                    canvas,
                    &grid,
                    avatar_rect,
                    if is_active {
                        theme.live_emerald()
                    } else if info.is_online {
                        theme.accent_rgb
                    } else {
                        0x94A3B8
                    },
                );

                let text_x = avatar_rect.x + avatar_rect.w + 10.0;
                canvas.draw_text(
                    &info.username,
                    text_x,
                    row_rect.y + 8.0,
                    theme.ink_primary(),
                    true,
                );
                let kind_txt = if info.is_online {
                    lang.text("mode.online")
                } else {
                    lang.text("mode.offline")
                };
                let subtitle = format!("{}  •  {kind_txt}", lang.text("launcher.tag"));
                canvas.draw_text(
                    &subtitle,
                    text_x,
                    row_rect.y + 22.0,
                    if info.is_online {
                        theme.live_emerald()
                    } else {
                        theme.ink_muted()
                    },
                    false,
                );

                let sw_rect = Rect::new(row_rect.x + row_rect.w - 70.0, row_rect.y + 11.0, 62.0, 22.0);
                GlassButtonWidget::new(
                    sw_rect,
                    if is_active {
                        lang.text("btn.active")
                    } else {
                        lang.text("btn.switch")
                    },
                    if is_active {
                        ButtonTone::Primary
                    } else {
                        ButtonTone::Secondary
                    },
                )
                .draw(canvas, theme, mouse);
                launcher_hit_rect = Some(row_rect);
            }
        }

        if rows_snapshot.is_empty() {
            let empty_txt = lang.text("empty.accounts");
            let tw = canvas.text_width(empty_txt);
            let empty_y = if launcher_card.is_some() {
                list_rect.y + 6.0 + row_h + 18.0
            } else {
                list_rect.y + list_rect.h * 0.5 - 4.0
            };
            draw_truncated_text(
                canvas,
                empty_txt,
                Rect::new(
                    list_rect.x + ((list_rect.w - tw) * 0.5).max(8.0),
                    empty_y,
                    list_rect.w - 16.0,
                    14.0,
                ),
                theme.ink_muted(),
                false,
            );
        } else {
            let now = unix_now();
            let active_id = self.service.active_account_id().map(str::to_owned);
            for (i, acc) in rows_snapshot.iter().enumerate() {
                let ry =
                    list_rect.y + 6.0 + (i + card_offset) as f32 * (row_h + row_gap) - self.scroll_y;
                if ry + row_h < list_rect.y || ry > list_rect.y + list_h {
                    continue;
                }
                let row_rect = Rect::new(list_rect.x + 6.0, ry, list_rect.w - 12.0, row_h);
                let is_active = active_id.as_deref() == Some(acc.id());
                let hovered = self.modal == ModalView::None
                    && list_rect.contains(mouse[0], mouse[1])
                    && row_rect.contains(mouse[0], mouse[1]);

                draw_rounded_rect(canvas, row_rect, 9.0, theme.card_fill(hovered, is_active));
                draw_rounded_outline(
                    canvas,
                    row_rect,
                    9.0,
                    1.0,
                    if is_active {
                        theme.live_emerald()
                    } else if hovered {
                        theme.border_highlight()
                    } else {
                        argb(255, 255, 255, 36)
                    },
                    with_alpha(theme.border_subtle(), 70),
                );

                // Avatar 8x8 composited grid
                let grid = self.service.avatar_for(acc);
                let avatar_rect = Rect::new(row_rect.x + 8.0, row_rect.y + 6.0, 32.0, 32.0);
                draw_avatar_grid(
                    canvas,
                    &grid,
                    avatar_rect,
                    if is_active {
                        theme.live_emerald()
                    } else if acc.is_online() {
                        theme.accent_rgb
                    } else {
                        0x94A3B8
                    },
                );

                // Account title + status badge
                let text_x = avatar_rect.x + avatar_rect.w + 10.0;
                canvas.draw_text(
                    acc.username(),
                    text_x,
                    row_rect.y + 8.0,
                    theme.ink_primary(),
                    true,
                );
                let kind_txt = if acc.is_online() {
                    lang.text("mode.online")
                } else {
                    lang.text("mode.offline")
                };
                let (st_txt, st_col) = match acc.status(now) {
                    AuthStatusBadge::Valid => (lang.text("status.valid"), theme.live_emerald()),
                    AuthStatusBadge::ExpiringSoon => {
                        (lang.text("status.expiring"), theme.warn_amber())
                    }
                    AuthStatusBadge::Expired => (lang.text("status.expired"), theme.danger_coral()),
                    AuthStatusBadge::Offline => (lang.text("status.offline"), theme.ink_muted()),
                };
                let subtitle = format!("{kind_txt}  •  {st_txt}");
                canvas.draw_text(&subtitle, text_x, row_rect.y + 22.0, st_col, false);

                // Action buttons inside card right edge
                let del_rect = Rect::new(row_rect.x + row_rect.w - 30.0, row_rect.y + 11.0, 22.0, 22.0);
                let del_label = if self.confirm_delete_id.as_deref() == Some(acc.id()) {
                    "!"
                } else {
                    "-"
                };
                GlassButtonWidget::new(del_rect, del_label, ButtonTone::Danger)
                    .draw(canvas, theme, mouse);

                let fav_rect = Rect::new(del_rect.x - 26.0, row_rect.y + 11.0, 22.0, 22.0);
                let fav_label = if acc.profile().favorite { "*" } else { "+" };
                GlassButtonWidget::new(
                    fav_rect,
                    fav_label,
                    if acc.profile().favorite {
                        ButtonTone::Primary
                    } else {
                        ButtonTone::Secondary
                    },
                )
                .draw(canvas, theme, mouse);

                let refresh_rect = if acc.is_online() {
                    let rr = Rect::new(fav_rect.x - 56.0, row_rect.y + 11.0, 52.0, 22.0);
                    GlassButtonWidget::new(rr, lang.text("btn.refresh"), ButtonTone::Secondary)
                        .draw(canvas, theme, mouse);
                    Some(rr)
                } else {
                    None
                };

                let switch_anchor_x = refresh_rect.map_or(fav_rect.x, |r| r.x);
                let sw_rect = Rect::new(switch_anchor_x - 62.0, row_rect.y + 11.0, 58.0, 22.0);
                GlassButtonWidget::new(
                    sw_rect,
                    if is_active {
                        lang.text("btn.active")
                    } else {
                        lang.text("btn.switch")
                    },
                    if is_active {
                        ButtonTone::Primary
                    } else {
                        ButtonTone::Secondary
                    },
                )
                .draw(canvas, theme, mouse);

                hit_cards.push(CardHitRects {
                    account_id: acc.id().to_owned(),
                    row_rect: Rect::new(row_rect.x, row_rect.y, switch_anchor_x - row_rect.x, row_rect.h),
                    favorite_rect: fav_rect,
                    refresh_rect,
                    delete_rect: del_rect,
                });
            }
        }
        canvas.end_clip(clip, list_rect);

        if self.modal == ModalView::None {
            if let Some(lr) = launcher_hit_rect
                && list_rect.contains(lr.x + 8.0, lr.y + 12.0)
            {
                self.last_hit_regions
                    .push((lr, ClickTarget::RestoreLauncherSession));
            }
            for card in hit_cards {
                if list_rect.contains(card.delete_rect.x + 2.0, card.delete_rect.y + 2.0) {
                    self.last_hit_regions.push((
                        card.delete_rect,
                        ClickTarget::DeleteAccount(card.account_id.clone()),
                    ));
                    self.last_hit_regions.push((
                        card.favorite_rect,
                        ClickTarget::ToggleFavorite(card.account_id.clone()),
                    ));
                    if let Some(rr) = card.refresh_rect {
                        self.last_hit_regions
                            .push((rr, ClickTarget::RefreshAccount(card.account_id.clone())));
                    }
                    self.last_hit_regions
                        .push((card.row_rect, ClickTarget::SwitchAccount(card.account_id)));
                }
            }
        }

        // Footer bar: banner feedback + Add Microsoft + Add Offline
        let foot_y = panel.y + panel.h - 34.0;
        let btn_ms = GlassButtonWidget::new(
            Rect::new(panel.x + pad, foot_y, 102.0, 22.0),
            lang.text("btn.add_microsoft"),
            ButtonTone::Primary,
        );
        btn_ms.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((btn_ms.rect, ClickTarget::OpenAddMicrosoft));

        let btn_off = GlassButtonWidget::new(
            Rect::new(btn_ms.rect.x + btn_ms.rect.w + 8.0, foot_y, 92.0, 22.0),
            lang.text("btn.add_offline"),
            ButtonTone::Secondary,
        );
        btn_off.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((btn_off.rect, ClickTarget::OpenAddOffline));

        if let Some((msg, is_err)) = &self.service.banner_message {
            let banner_x = btn_off.rect.x + btn_off.rect.w + 12.0;
            let banner_w = (panel.x + panel.w - pad - banner_x).max(0.0);
            draw_truncated_text(
                canvas,
                msg,
                Rect::new(banner_x, foot_y + 6.5, banner_w, 14.0),
                if *is_err {
                    theme.danger_coral()
                } else {
                    theme.live_emerald()
                },
                false,
            );
        } else {
            let hint = lang.text("shortcut.hint");
            let hw = canvas.text_width(hint);
            let hint_x = (panel.x + panel.w - pad - hw).max(btn_off.rect.x + btn_off.rect.w + 10.0);
            let hint_w = (panel.x + panel.w - pad - hint_x).max(0.0);
            draw_truncated_text(
                canvas,
                hint,
                Rect::new(hint_x, foot_y + 6.5, hint_w, 14.0),
                theme.ink_faint(),
                false,
            );
        }
    }

    fn render_add_offline_modal(
        &mut self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        lang: &LanguagePack,
        parent: Rect,
        mouse: [f32; 2],
    ) {
        self.last_hit_regions.clear();
        let box_rect = Rect::new(
            parent.x + (parent.w - 300.0) * 0.5,
            parent.y + (parent.h - 142.0) * 0.5,
            300.0,
            142.0,
        );
        draw_liquid_glass_panel(canvas, theme, box_rect, 14.0);
        canvas.draw_text(
            lang.text("modal.offline.title"),
            box_rect.x + 16.0,
            box_rect.y + 14.0,
            theme.ink_primary(),
            true,
        );
        let field_rect = Rect::new(box_rect.x + 16.0, box_rect.y + 42.0, box_rect.w - 32.0, 24.0);
        self.offline_name_field.focused = self.active_field == ActiveField::OfflineName;
        self.offline_name_field.render_in(
            canvas,
            theme,
            field_rect,
            lang.text("modal.offline.hint"),
        );
        self.last_hit_regions.push((
            field_rect,
            ClickTarget::FocusField(ActiveField::OfflineName),
        ));

        let uuid_preview = if self.offline_name_field.value.is_empty() {
            String::new()
        } else {
            let u = crate::auth::offline_uuid_for(self.offline_name_field.value.trim());
            format!("UUID: {}", crate::domain::format_uuid_dashed(u))
        };
        if !uuid_preview.is_empty() {
            draw_truncated_text(
                canvas,
                &uuid_preview,
                Rect::new(box_rect.x + 16.0, box_rect.y + 74.0, box_rect.w - 32.0, 14.0),
                theme.ink_muted(),
                false,
            );
        }
        if let Some((err, true)) = &self.service.banner_message {
            draw_truncated_text(
                canvas,
                err,
                Rect::new(box_rect.x + 16.0, box_rect.y + 88.0, box_rect.w - 32.0, 14.0),
                theme.danger_coral(),
                false,
            );
        }

        let confirm = GlassButtonWidget::new(
            Rect::new(box_rect.x + box_rect.w - 104.0, box_rect.y + 104.0, 88.0, 22.0),
            lang.text("btn.confirm"),
            ButtonTone::Primary,
        );
        confirm.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((confirm.rect, ClickTarget::ConfirmAddOffline));

        let cancel = GlassButtonWidget::new(
            Rect::new(box_rect.x + 16.0, box_rect.y + 104.0, 82.0, 22.0),
            lang.text("btn.cancel"),
            ButtonTone::Secondary,
        );
        cancel.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((cancel.rect, ClickTarget::CloseModal));
    }

    fn render_add_microsoft_modal(
        &mut self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        lang: &LanguagePack,
        parent: Rect,
        method: OAuthLoginKind,
        mouse: [f32; 2],
    ) {
        self.last_hit_regions.clear();
        let box_rect = Rect::new(
            parent.x + (parent.w - 380.0) * 0.5,
            parent.y + (parent.h - 224.0) * 0.5,
            380.0,
            224.0,
        );
        draw_liquid_glass_panel(canvas, theme, box_rect, 14.0);
        canvas.draw_text(
            lang.text("modal.ms.title"),
            box_rect.x + 16.0,
            box_rect.y + 14.0,
            theme.ink_primary(),
            true,
        );

        let b_dev = GlassButtonWidget::new(
            Rect::new(box_rect.x + 16.0, box_rect.y + 38.0, 170.0, 22.0),
            lang.text("modal.ms.device"),
            if method == OAuthLoginKind::DeviceCode {
                ButtonTone::Primary
            } else {
                ButtonTone::Secondary
            },
        );
        b_dev.draw(canvas, theme, mouse);
        self.last_hit_regions.push((
            b_dev.rect,
            ClickTarget::SelectMsMethod(OAuthLoginKind::DeviceCode),
        ));

        let b_pkce = GlassButtonWidget::new(
            Rect::new(b_dev.rect.x + 178.0, box_rect.y + 38.0, 170.0, 22.0),
            lang.text("modal.ms.browser"),
            if method == OAuthLoginKind::BrowserPkce {
                ButtonTone::Primary
            } else {
                ButtonTone::Secondary
            },
        );
        b_pkce.draw(canvas, theme, mouse);
        self.last_hit_regions.push((
            b_pkce.rect,
            ClickTarget::SelectMsMethod(OAuthLoginKind::BrowserPkce),
        ));

        let content_top = if method == OAuthLoginKind::BrowserPkce {
            let cid_rect = Rect::new(box_rect.x + 16.0, box_rect.y + 70.0, box_rect.w - 32.0, 22.0);
            self.ms_client_id_field.focused = self.active_field == ActiveField::MsClientId;
            self.ms_client_id_field.render_in(
                canvas,
                theme,
                cid_rect,
                lang.text("modal.ms.client_id"),
            );
            self.last_hit_regions
                .push((cid_rect, ClickTarget::FocusField(ActiveField::MsClientId)));
            box_rect.y + 102.0
        } else {
            box_rect.y + 74.0
        };
        let status_bounds = Rect::new(
            box_rect.x + 16.0,
            content_top,
            box_rect.w - 32.0,
            (box_rect.y + 180.0 - content_top).max(20.0),
        );

        match &self.service.auth_job {
            AuthJobState::Idle => {
                let hint = if method == OAuthLoginKind::DeviceCode {
                    lang.text("modal.ms.device_hint")
                } else {
                    lang.text("modal.ms.browser_hint")
                };
                draw_wrapped_text(canvas, hint, status_bounds, 13.0, theme.ink_muted(), false);
            }
            AuthJobState::Running {
                stage,
                device_code,
                browser_url: _,
            } => {
                draw_truncated_text(
                    canvas,
                    stage,
                    Rect::new(status_bounds.x, status_bounds.y, status_bounds.w, 14.0),
                    theme.accent_soft(),
                    false,
                );
                if let Some(dc) = device_code {
                    let code_txt = format!("{}  {}", lang.text("modal.ms.copy_code"), dc.user_code);
                    draw_truncated_text(
                        canvas,
                        &code_txt,
                        Rect::new(status_bounds.x, status_bounds.y + 20.0, status_bounds.w, 15.0),
                        theme.live_emerald(),
                        true,
                    );
                    let copy_btn = GlassButtonWidget::new(
                        Rect::new(status_bounds.x, status_bounds.y + 42.0, 140.0, 22.0),
                        lang.text("modal.ms.copy_btn"),
                        ButtonTone::Secondary,
                    );
                    copy_btn.draw(canvas, theme, mouse);
                    self.last_hit_regions.push((
                        copy_btn.rect,
                        ClickTarget::CopyDeviceCode(dc.user_code.clone()),
                    ));

                    let link_btn = GlassButtonWidget::new(
                        Rect::new(status_bounds.x + 148.0, status_bounds.y + 42.0, status_bounds.w - 148.0, 22.0),
                        &dc.verification_uri,
                        ButtonTone::Secondary,
                    );
                    link_btn.draw(canvas, theme, mouse);
                    self.last_hit_regions.push((
                        link_btn.rect,
                        ClickTarget::OpenDeviceLink(dc.verification_uri.clone()),
                    ));
                }
            }
            AuthJobState::Succeeded(name) => {
                draw_truncated_text(
                    canvas,
                    &format!("Signed in: {name}"),
                    Rect::new(status_bounds.x, status_bounds.y, status_bounds.w, 15.0),
                    theme.live_emerald(),
                    true,
                );
            }
            AuthJobState::Failed(err) => {
                draw_wrapped_text(canvas, err, status_bounds, 13.0, theme.danger_coral(), false);
            }
        }

        let start = GlassButtonWidget::new(
            Rect::new(box_rect.x + box_rect.w - 130.0, box_rect.y + 186.0, 114.0, 22.0),
            lang.text("modal.ms.start"),
            ButtonTone::Primary,
        )
        .with_enabled(!matches!(self.service.auth_job, AuthJobState::Running { .. }));
        start.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((start.rect, ClickTarget::StartMsLogin));

        let cancel = GlassButtonWidget::new(
            Rect::new(box_rect.x + 16.0, box_rect.y + 186.0, 86.0, 22.0),
            lang.text("btn.cancel"),
            ButtonTone::Secondary,
        );
        cancel.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((cancel.rect, ClickTarget::CloseModal));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_vault_settings_modal(
        &mut self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        lang: &LanguagePack,
        parent: Rect,
        mode: EncryptionMode,
        auto_refresh: bool,
        show_title_badge: bool,
        show_mp_badge: bool,
        mouse: [f32; 2],
    ) {
        self.last_hit_regions.clear();
        let box_rect = Rect::new(
            parent.x + (parent.w - 376.0) * 0.5,
            parent.y + (parent.h - 246.0) * 0.5,
            376.0,
            246.0,
        );
        draw_liquid_glass_panel(canvas, theme, box_rect, 14.0);
        canvas.draw_text(
            lang.text("modal.vault.title"),
            box_rect.x + 16.0,
            box_rect.y + 14.0,
            theme.ink_primary(),
            true,
        );

        let modes = [
            (EncryptionMode::DeviceKey, lang.text("modal.vault.device_key")),
            (EncryptionMode::PasswordProtected, lang.text("modal.vault.password")),
            (EncryptionMode::Plaintext, lang.text("modal.vault.plaintext")),
        ];
        let mut my = box_rect.y + 38.0;
        for (m, label) in modes {
            let b = GlassButtonWidget::new(
                Rect::new(box_rect.x + 16.0, my, box_rect.w - 32.0, 20.0),
                label,
                if mode == m {
                    ButtonTone::Primary
                } else {
                    ButtonTone::Secondary
                },
            );
            b.draw(canvas, theme, mouse);
            self.last_hit_regions
                .push((b.rect, ClickTarget::SelectEncryptionMode(m)));
            my += 24.0;
        }

        let pw_rect = Rect::new(box_rect.x + 16.0, my + 4.0, box_rect.w - 32.0, 22.0);
        self.vault_password_field.focused = self.active_field == ActiveField::VaultPassword;
        self.vault_password_field.render_in(
            canvas,
            theme,
            pw_rect,
            lang.text("modal.vault.pass_hint"),
        );
        self.last_hit_regions
            .push((pw_rect, ClickTarget::FocusField(ActiveField::VaultPassword)));

        let cid_rect = Rect::new(box_rect.x + 16.0, pw_rect.y + 28.0, box_rect.w - 32.0, 22.0);
        self.vault_client_id_field.focused = self.active_field == ActiveField::VaultClientId;
        self.vault_client_id_field.render_in(
            canvas,
            theme,
            cid_rect,
            lang.text("modal.ms.client_id"),
        );
        self.last_hit_regions
            .push((cid_rect, ClickTarget::FocusField(ActiveField::VaultClientId)));

        let toggles = [
            (
                format!("Auto-Refresh: {}", if auto_refresh { "ON" } else { "OFF" }),
                ClickTarget::ToggleAutoRefresh,
            ),
            (
                format!("Title Pill: {}", if show_title_badge { "ON" } else { "OFF" }),
                ClickTarget::ToggleTitleBadge,
            ),
            (
                format!("MP Pill: {}", if show_mp_badge { "ON" } else { "OFF" }),
                ClickTarget::ToggleMpBadge,
            ),
        ];
        let mut tx = box_rect.x + 16.0;
        let ty = cid_rect.y + 30.0;
        for (lbl, tgt) in toggles {
            let tb = GlassButtonWidget::new(Rect::new(tx, ty, 110.0, 20.0), lbl, ButtonTone::Secondary);
            tb.draw(canvas, theme, mouse);
            self.last_hit_regions.push((tb.rect, tgt));
            tx += 116.0;
        }

        let save = GlassButtonWidget::new(
            Rect::new(box_rect.x + box_rect.w - 106.0, box_rect.y + 210.0, 90.0, 22.0),
            lang.text("btn.confirm"),
            ButtonTone::Primary,
        );
        save.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((save.rect, ClickTarget::SaveVaultSettings));

        let cancel = GlassButtonWidget::new(
            Rect::new(box_rect.x + 16.0, box_rect.y + 210.0, 82.0, 22.0),
            lang.text("btn.cancel"),
            ButtonTone::Secondary,
        );
        cancel.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((cancel.rect, ClickTarget::CloseModal));
    }

    fn render_unlock_modal(
        &mut self,
        canvas: &mut dyn UiCanvas,
        theme: &LiquidGlassTheme,
        lang: &LanguagePack,
        parent: Rect,
        mouse: [f32; 2],
    ) {
        self.last_hit_regions.clear();
        let box_rect = Rect::new(
            parent.x + (parent.w - 300.0) * 0.5,
            parent.y + (parent.h - 134.0) * 0.5,
            300.0,
            134.0,
        );
        draw_liquid_glass_panel(canvas, theme, box_rect, 14.0);
        canvas.draw_text(
            lang.text("modal.unlock.title"),
            box_rect.x + 16.0,
            box_rect.y + 14.0,
            theme.ink_primary(),
            true,
        );

        let pw_rect = Rect::new(box_rect.x + 16.0, box_rect.y + 44.0, box_rect.w - 32.0, 24.0);
        self.unlock_password_field.focused = self.active_field == ActiveField::UnlockPassword;
        self.unlock_password_field.render_in(
            canvas,
            theme,
            pw_rect,
            lang.text("modal.unlock.hint"),
        );
        self.last_hit_regions
            .push((pw_rect, ClickTarget::FocusField(ActiveField::UnlockPassword)));

        if let Some((err, true)) = &self.service.banner_message {
            draw_truncated_text(
                canvas,
                err,
                Rect::new(box_rect.x + 16.0, box_rect.y + 74.0, box_rect.w - 32.0, 14.0),
                theme.danger_coral(),
                false,
            );
        }

        let unlock = GlassButtonWidget::new(
            Rect::new(box_rect.x + box_rect.w - 104.0, box_rect.y + 96.0, 88.0, 22.0),
            lang.text("btn.confirm"),
            ButtonTone::Primary,
        );
        unlock.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((unlock.rect, ClickTarget::SubmitUnlock));

        let cancel = GlassButtonWidget::new(
            Rect::new(box_rect.x + 16.0, box_rect.y + 96.0, 82.0, 22.0),
            lang.text("btn.close"),
            ButtonTone::Secondary,
        );
        cancel.draw(canvas, theme, mouse);
        self.last_hit_regions
            .push((cancel.rect, ClickTarget::CloseOverlay));
    }

    /// Handles left/right mouse clicks; returns `true` when the click was consumed by our UI.
    pub fn handle_mouse_press(&mut self, mouse: [f32; 2], button: i32) -> bool {
        if button != 0 {
            return self.open;
        }
        let target = self
            .last_hit_regions
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(mouse[0], mouse[1]))
            .map(|(_, t)| t.clone());

        let Some(target) = target else {
            return self.open;
        };

        match target {
            ClickTarget::OpenOverlay => self.open_overlay(),
            ClickTarget::CloseOverlay => self.close_overlay(),
            ClickTarget::Filter(f) => {
                self.filter = f;
                self.scroll_y = 0.0;
            }
            ClickTarget::FocusField(field) => self.active_field = field,
            ClickTarget::OpenAddMicrosoft => {
                self.ms_client_id_field.value = self.service.settings().custom_client_id.clone();
                if matches!(self.service.auth_job, AuthJobState::Failed(_) | AuthJobState::Succeeded(_)) {
                    self.service.auth_job = AuthJobState::Idle;
                }
                self.modal = ModalView::AddMicrosoft {
                    method: OAuthLoginKind::DeviceCode,
                };
                self.active_field = ActiveField::MsClientId;
            }
            ClickTarget::OpenAddOffline => {
                self.offline_name_field.clear();
                self.service.banner_message = None;
                self.modal = ModalView::AddOffline;
                self.active_field = ActiveField::OfflineName;
            }
            ClickTarget::OpenVaultSettings => {
                let s = self.service.settings().clone();
                self.vault_client_id_field.value = s.custom_client_id;
                self.vault_password_field.clear();
                self.modal = ModalView::VaultSettings {
                    selected_mode: self.service.encryption_mode(),
                    auto_refresh: s.auto_refresh_tokens,
                    show_title_badge: s.show_title_badge,
                    show_mp_badge: s.show_multiplayer_badge,
                };
                self.active_field = ActiveField::VaultClientId;
            }
            ClickTarget::RestoreLauncherSession => {
                if let Err(err) = self.service.restore_launcher_session() {
                    self.service.banner_message = Some((err.to_string(), true));
                }
            }
            ClickTarget::SwitchAccount(id) => {
                if let Err(err) = self.service.switch_to(&id) {
                    self.service.banner_message = Some((err.to_string(), true));
                }
            }
            ClickTarget::ToggleFavorite(id) => {
                let _ = self.service.toggle_favorite(&id);
            }
            ClickTarget::RefreshAccount(id) => {
                if let Err(err) = self.service.refresh_account(&id) {
                    self.service.banner_message = Some((err.to_string(), true));
                }
            }
            ClickTarget::DeleteAccount(id) => {
                if self.confirm_delete_id.as_deref() == Some(id.as_str()) {
                    let _ = self.service.delete_account(&id);
                    self.confirm_delete_id = None;
                } else {
                    self.confirm_delete_id = Some(id);
                }
            }
            ClickTarget::ConfirmAddOffline => self.submit_add_offline(),
            ClickTarget::CloseModal => {
                self.service.cancel_auth_job();
                self.modal = ModalView::None;
                self.active_field = ActiveField::Search;
            }
            ClickTarget::SelectMsMethod(m) => {
                if !matches!(self.service.auth_job, AuthJobState::Running { .. }) {
                    self.service.auth_job = AuthJobState::Idle;
                }
                self.modal = ModalView::AddMicrosoft { method: m };
            }
            ClickTarget::StartMsLogin => {
                if let ModalView::AddMicrosoft { method } = self.modal {
                    let cid = self.ms_client_id_field.value.clone();
                    if let Err(err) = self.service.start_microsoft_login(method, &cid) {
                        self.service.auth_job = AuthJobState::Failed(err.to_string());
                    }
                }
            }
            ClickTarget::OpenDeviceLink(url) => {
                let _ = open::that_detached(url);
            }
            ClickTarget::CopyDeviceCode(code) => {
                self.clipboard.set_text(&code);
                self.service.banner_message = Some(("Код скопирован в буфер обмена!".into(), false));
            }
            ClickTarget::SelectEncryptionMode(m) => {
                if let ModalView::VaultSettings {
                    auto_refresh,
                    show_title_badge,
                    show_mp_badge,
                    ..
                } = self.modal
                {
                    self.modal = ModalView::VaultSettings {
                        selected_mode: m,
                        auto_refresh,
                        show_title_badge,
                        show_mp_badge,
                    };
                }
            }
            ClickTarget::ToggleAutoRefresh => {
                if let ModalView::VaultSettings {
                    selected_mode,
                    auto_refresh,
                    show_title_badge,
                    show_mp_badge,
                } = self.modal
                {
                    self.modal = ModalView::VaultSettings {
                        selected_mode,
                        auto_refresh: !auto_refresh,
                        show_title_badge,
                        show_mp_badge,
                    };
                }
            }
            ClickTarget::ToggleTitleBadge => {
                if let ModalView::VaultSettings {
                    selected_mode,
                    auto_refresh,
                    show_title_badge,
                    show_mp_badge,
                } = self.modal
                {
                    self.modal = ModalView::VaultSettings {
                        selected_mode,
                        auto_refresh,
                        show_title_badge: !show_title_badge,
                        show_mp_badge,
                    };
                }
            }
            ClickTarget::ToggleMpBadge => {
                if let ModalView::VaultSettings {
                    selected_mode,
                    auto_refresh,
                    show_title_badge,
                    show_mp_badge,
                } = self.modal
                {
                    self.modal = ModalView::VaultSettings {
                        selected_mode,
                        auto_refresh,
                        show_title_badge,
                        show_mp_badge: !show_mp_badge,
                    };
                }
            }
            ClickTarget::SaveVaultSettings => self.submit_vault_settings(),
            ClickTarget::SubmitUnlock => self.submit_unlock(),
        }
        true
    }

    /// Consumes mouse wheel scroll when overlay is open.
    pub fn handle_mouse_scroll(&mut self, scroll_y: f64) -> bool {
        if !self.open {
            return false;
        }
        if self.modal == ModalView::None {
            self.scroll_y = (self.scroll_y - scroll_y as f32 * 18.0).clamp(0.0, self.max_scroll);
        }
        true
    }

    /// Pastes text into the active input field (ignoring line breaks and honoring `max_len`).
    pub fn paste_text(&mut self, text: &str) -> bool {
        if !self.open {
            return false;
        }
        if let Some(field) = self.current_field_mut() {
            for ch in text.chars() {
                if !matches!(ch, '\r' | '\n') && !ch.is_control() {
                    field.push_char(ch);
                }
            }
            return true;
        }
        false
    }

    /// Consumes `Alt+A` shortcut globally on screens and handles Escape / Backspace / Ctrl+V / Enter / Title typing inside overlay.
    pub fn handle_key_press(&mut self, key: &str, modifiers: u32, screen_id: &str) -> bool {
        let on_supported_screen =
            matches!(screen_id, "title" | "multiplayer" | "world_select" | "paused");
        if (modifiers & KEY_ALT_MASK) != 0 && key == KEY_A && on_supported_screen {
            if self.open {
                self.close_overlay();
            } else {
                self.open_overlay();
            }
            return true;
        }

        if !self.open {
            return false;
        }

        if key == KEY_ESCAPE {
            if self.modal != ModalView::None && self.service.is_unlocked() {
                self.service.cancel_auth_job();
                self.modal = ModalView::None;
                self.active_field = ActiveField::Search;
            } else {
                self.close_overlay();
            }
            return true;
        }

        let ctrl_only = (modifiers & KEY_CTRL_MASK) != 0 && (modifiers & KEY_ALT_MASK) == 0;
        if ctrl_only {
            if key == KEY_V {
                let text = self.clipboard.get_text();
                self.paste_text(&text);
                return true;
            }
            if key == KEY_A {
                if let Some(field) = self.current_field_mut() {
                    field.clear();
                }
                return true;
            }
            return true;
        }

        if key == KEY_BACKSPACE || key == KEY_DELETE {
            if let Some(field) = self.current_field_mut() {
                field.backspace();
            }
            return true;
        }

        if key == KEY_ENTER || key == KEY_KP_ENTER {
            match self.modal {
                ModalView::AddOffline => self.submit_add_offline(),
                ModalView::UnlockVault => self.submit_unlock(),
                ModalView::VaultSettings { .. } => self.submit_vault_settings(),
                _ => {}
            }
            return true;
        }

        // Title-owned screens (where `title_owns_text` suppresses `screen_chars_typed` when canceled)
        // need key-to-char synthesis; other screens (`options`, `pause`, etc.) deliver `handle_char_typed`.
        if is_title_owned_screen(screen_id)
            && (modifiers & KEY_ALT_MASK) == 0
            && let Some(ch) = map_key_to_char(key, (modifiers & KEY_SHIFT_MASK) != 0)
        {
            self.handle_char_typed(ch);
        }

        true
    }

    /// Feeds printable typed characters into the focused liquid-glass input field.
    pub fn handle_char_typed(&mut self, ch: char) -> bool {
        if !self.open {
            return false;
        }
        if let Some(field) = self.current_field_mut() {
            field.push_char(ch);
        }
        true
    }

    fn current_field_mut(&mut self) -> Option<&mut GlassTextField> {
        match (self.modal.clone(), self.active_field) {
            (ModalView::None, ActiveField::Search) => Some(&mut self.search_field),
            (ModalView::AddOffline, ActiveField::OfflineName) => Some(&mut self.offline_name_field),
            (ModalView::AddMicrosoft { .. }, ActiveField::MsClientId) => {
                Some(&mut self.ms_client_id_field)
            }
            (ModalView::VaultSettings { .. }, ActiveField::VaultPassword) => {
                Some(&mut self.vault_password_field)
            }
            (ModalView::VaultSettings { .. }, ActiveField::VaultClientId) => {
                Some(&mut self.vault_client_id_field)
            }
            (ModalView::UnlockVault, ActiveField::UnlockPassword) => {
                Some(&mut self.unlock_password_field)
            }
            _ => None,
        }
    }

    fn submit_add_offline(&mut self) {
        let name = self.offline_name_field.value.clone();
        match self.service.add_offline_account(&name, true) {
            Ok(_) => {
                self.offline_name_field.clear();
                self.modal = ModalView::None;
                self.active_field = ActiveField::Search;
            }
            Err(err) => {
                self.service.banner_message = Some((err.to_string(), true));
            }
        }
    }

    fn submit_vault_settings(&mut self) {
        let ModalView::VaultSettings {
            selected_mode,
            auto_refresh,
            show_title_badge,
            show_mp_badge,
        } = self.modal
        else {
            return;
        };
        let mut settings = self.service.settings().clone();
        settings.custom_client_id = self.vault_client_id_field.value.trim().to_owned();
        settings.auto_refresh_tokens = auto_refresh;
        settings.show_title_badge = show_title_badge;
        settings.show_multiplayer_badge = show_mp_badge;

        let pw = if self.vault_password_field.value.is_empty() {
            None
        } else {
            Some(self.vault_password_field.value.as_str())
        };
        match self
            .service
            .update_security_and_settings(selected_mode, pw, settings)
        {
            Ok(()) => {
                self.vault_password_field.clear();
                self.modal = ModalView::None;
                self.active_field = ActiveField::Search;
            }
            Err(err) => {
                self.service.banner_message = Some((err.to_string(), true));
            }
        }
    }

    fn submit_unlock(&mut self) {
        let pw = self.unlock_password_field.value.clone();
        match self.service.unlock(&pw) {
            Ok(()) => {
                self.unlock_password_field.clear();
                self.modal = ModalView::None;
                self.active_field = ActiveField::Search;
            }
            Err(err) => {
                self.service.banner_message = Some((err.to_string(), true));
            }
        }
    }
}

fn lang_label<'a>(lang_code: &str, ru: &'a str, en: &'a str) -> &'a str {
    if Locale::from_game_code(lang_code) == Locale::RuRu {
        ru
    } else {
        en
    }
}

fn is_title_owned_screen(screen_id: &str) -> bool {
    matches!(
        screen_id,
        "title"
            | "realms"
            | "world_select"
            | "world_create"
            | "world_create_flat"
            | "world_create_buffet"
            | "world_edit"
            | "world_backup"
            | "world_backup_confirm"
            | "world_optimize_confirm"
            | "world_optimize"
            | "low_disk_space"
            | "world_delete"
            | "world_recover"
            | "world_recovered"
            | "error"
            | "multiplayer"
            | "add_server"
            | "edit_server"
            | "direct_connect"
            | "server_delete"
            | "friends_join"
            | "disconnected"
            | "multiplayer_warning"
            | "onboarding"
            | "ban_notice"
    )
}

fn map_key_to_char(key: &str, shift: bool) -> Option<char> {
    let suffix = key.strip_prefix("key.keyboard.")?;
    if suffix.len() == 1 {
        let b = suffix.as_bytes()[0];
        if b.is_ascii_lowercase() {
            return Some(if shift { b.to_ascii_uppercase() as char } else { b as char });
        }
        if b.is_ascii_digit() {
            return Some(if shift {
                match b {
                    b'1' => '!',
                    b'2' => '@',
                    b'3' => '#',
                    b'4' => '$',
                    b'5' => '%',
                    b'6' => '^',
                    b'7' => '&',
                    b'8' => '*',
                    b'9' => '(',
                    b'0' => ')',
                    _ => b as char,
                }
            } else {
                b as char
            });
        }
    }
    Some(match (suffix, shift) {
        ("space", _) => ' ',
        ("minus", false) => '-',
        ("minus", true) => '_',
        ("equal", false) => '=',
        ("equal", true) => '+',
        ("left.bracket", false) => '[',
        ("left.bracket", true) => '{',
        ("right.bracket", false) => ']',
        ("right.bracket", true) => '}',
        ("backslash", false) => '\\',
        ("backslash", true) => '|',
        ("semicolon", false) => ';',
        ("semicolon", true) => ':',
        ("apostrophe", false) => '\'',
        ("apostrophe", true) => '"',
        ("grave.accent", false) => '`',
        ("grave.accent", true) => '~',
        ("comma", false) => ',',
        ("comma", true) => '<',
        ("period", false) => '.',
        ("period", true) => '>',
        ("slash", false) => '/',
        ("slash", true) => '?',
        ("keypad.0", _) => '0',
        ("keypad.1", _) => '1',
        ("keypad.2", _) => '2',
        ("keypad.3", _) => '3',
        ("keypad.4", _) => '4',
        ("keypad.5", _) => '5',
        ("keypad.6", _) => '6',
        ("keypad.7", _) => '7',
        ("keypad.8", _) => '8',
        ("keypad.9", _) => '9',
        ("keypad.period", _) => '.',
        ("keypad.comma", _) => ',',
        ("keypad.divide", _) => '/',
        ("keypad.multiply", _) => '*',
        ("keypad.subtract", _) => '-',
        ("keypad.add", _) => '+',
        ("keypad.equal", _) => '=',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::SwitcherError, session::SessionBridge, storage::FileAccountRepository};
    use std::sync::{Arc, Mutex};

    struct TestBridge {
        launcher_name: String,
        name: Mutex<(String, bool)>,
    }

    impl TestBridge {
        fn new(name: &str) -> Self {
            Self {
                launcher_name: name.to_owned(),
                name: Mutex::new((name.to_owned(), true)),
            }
        }
    }

    impl SessionBridge for TestBridge {
        fn apply_online(
            &self,
            username: &str,
            _uuid: u128,
            _xuid: Option<&str>,
            _access_token: &str,
        ) -> Result<(), SwitcherError> {
            *self.name.lock().unwrap() = (username.to_owned(), true);
            Ok(())
        }

        fn apply_offline(&self, username: &str) -> Result<(), SwitcherError> {
            *self.name.lock().unwrap() = (username.to_owned(), false);
            Ok(())
        }

        fn restore_launcher(&self) -> Result<(), SwitcherError> {
            *self.name.lock().unwrap() = (self.launcher_name.clone(), true);
            Ok(())
        }

        fn current_identity(&self) -> (String, bool) {
            self.name.lock().unwrap().clone()
        }

        fn launcher_session(&self) -> Option<crate::LauncherSessionInfo> {
            Some(crate::LauncherSessionInfo {
                username: self.launcher_name.clone(),
                is_online: true,
                uuid: Some(0xABCDEF),
            })
        }
    }

    struct MockCanvas {
        drawn_texts: Vec<String>,
        rects: usize,
        triangles: usize,
    }

    impl MockCanvas {
        fn new() -> Self {
            Self {
                drawn_texts: Vec::new(),
                rects: 0,
                triangles: 0,
            }
        }
    }

    impl UiCanvas for MockCanvas {
        fn screen_size(&self) -> (f32, f32) {
            (854.0, 480.0)
        }

        fn time_secs(&self) -> f32 {
            1.0
        }

        fn fill_rect(&mut self, _rect: Rect, _color: u32) {
            self.rects += 1;
        }

        fn fill_gradient(&mut self, _rect: Rect, _top: u32, _bottom: u32) {
            self.rects += 1;
        }

        fn fill_triangle(&mut self, _p0: [f32; 2], _p1: [f32; 2], _p2: [f32; 2], _color: u32) {
            self.triangles += 1;
        }

        fn draw_text(&mut self, text: &str, _x: f32, _y: f32, _color: u32, _shadow: bool) {
            self.drawn_texts.push(text.to_owned());
        }

        fn text_width(&self, text: &str) -> f32 {
            text.len() as f32 * 6.0
        }

        fn begin_clip(&mut self) -> usize {
            0
        }

        fn end_clip(&mut self, _start_vertex: usize, _rect: Rect) {}
    }

    #[test]
    fn renders_pill_and_drives_offline_creation_via_ui_events() {
        let dir = std::env::temp_dir().join(format!("horizon_as_ui_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = Arc::new(TestBridge::new("BootPlayer"));
        let repo = Box::new(FileAccountRepository::in_dir(&dir));
        let svc = AccountSwitcherService::bootstrap(repo, bridge.clone());
        let mut ctrl = AccountSwitcherController::new(svc);
        let mut canvas = MockCanvas::new();

        // 1. Pill renders on title screen
        ctrl.render(&mut canvas, "title", [0.0, 0.0], "en_us");
        assert!(canvas.drawn_texts.iter().any(|t| t.contains("BootPlayer")));

        // 2. Alt+A opens overlay
        assert!(ctrl.handle_key_press(KEY_A, KEY_ALT_MASK, "title"));
        assert!(ctrl.is_open());

        canvas.drawn_texts.clear();
        ctrl.render(&mut canvas, "title", [400.0, 240.0], "ru_ru");
        assert!(canvas.rects > 5 && canvas.triangles > 10);
        assert!(
            canvas
                .drawn_texts
                .iter()
                .any(|t| t == "Переключатель аккаунтов")
        );

        // 3. Click "+ Оффлайн", type nickname, press Enter -> switches immediately
        let add_btn_pos = ctrl
            .last_hit_regions
            .iter()
            .find(|(_, t)| matches!(t, ClickTarget::OpenAddOffline))
            .map(|(r, _)| [r.x + 4.0, r.y + 4.0])
            .unwrap();
        assert!(ctrl.handle_mouse_press(add_btn_pos, 0));
        for ch in "GlassSteve".chars() {
            assert!(ctrl.handle_char_typed(ch));
        }
        assert!(ctrl.handle_key_press(KEY_ENTER, 0, "title"));
        assert_eq!(*bridge.name.lock().unwrap(), ("GlassSteve".into(), false));

        // 4. Esc closes overlay cleanly
        assert!(ctrl.handle_key_press(KEY_ESCAPE, 0, "title"));
        assert!(!ctrl.is_open());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn types_on_title_screen_pastes_text_and_opens_device_code_ms_modal() {
        let dir = std::env::temp_dir().join(format!("horizon_as_title_input_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bridge = Arc::new(TestBridge::new("BootPlayer"));
        let repo = Box::new(FileAccountRepository::in_dir(&dir));
        let svc = AccountSwitcherService::bootstrap(repo, bridge.clone());
        let mut ctrl = AccountSwitcherController::new(svc);
        let mut canvas = MockCanvas::new();

        ctrl.open_overlay();
        ctrl.render(&mut canvas, "title", [0.0, 0.0], "ru_ru");

        let off_pos = ctrl
            .last_hit_regions
            .iter()
            .find(|(_, t)| matches!(t, ClickTarget::OpenAddOffline))
            .map(|(r, _)| [r.x + 4.0, r.y + 4.0])
            .unwrap();
        assert!(ctrl.handle_mouse_press(off_pos, 0));

        // Title screen synthesizes ASCII + Shift+minus ('_') directly from key_press
        assert!(ctrl.handle_key_press("key.keyboard.c", KEY_SHIFT_MASK, "title"));
        assert!(ctrl.handle_key_press("key.keyboard.minus", KEY_SHIFT_MASK, "title"));
        assert!(ctrl.handle_key_press("key.keyboard.1", 0, "title"));
        assert!(ctrl.paste_text("Pro\n"));
        assert_eq!(ctrl.offline_name_field.value, "C_1Pro");

        // Enter submits offline account on title screen
        assert!(ctrl.handle_key_press(KEY_ENTER, 0, "title"));
        assert_eq!(*bridge.name.lock().unwrap(), ("C_1Pro".into(), false));

        // Clicking Launcher Session card switches cleanly back to initial launcher online session
        ctrl.render(&mut canvas, "title", [0.0, 0.0], "ru_ru");
        let launcher_pos = ctrl
            .last_hit_regions
            .iter()
            .find(|(_, t)| matches!(t, ClickTarget::RestoreLauncherSession))
            .map(|(r, _)| [r.x + 8.0, r.y + 8.0])
            .unwrap();
        assert!(ctrl.handle_mouse_press(launcher_pos, 0));
        assert_eq!(*bridge.name.lock().unwrap(), ("BootPlayer".into(), true));

        // Switching back to C_1Pro then double-clicking Delete restores BootPlayer automatically
        let switch_pos = ctrl
            .last_hit_regions
            .iter()
            .find(|(_, t)| matches!(t, ClickTarget::SwitchAccount(_)))
            .map(|(r, _)| [r.x + 8.0, r.y + 8.0])
            .unwrap();
        assert!(ctrl.handle_mouse_press(switch_pos, 0));
        assert_eq!(*bridge.name.lock().unwrap(), ("C_1Pro".into(), false));

        ctrl.render(&mut canvas, "title", [0.0, 0.0], "ru_ru");
        let del_pos = ctrl
            .last_hit_regions
            .iter()
            .find(|(_, t)| matches!(t, ClickTarget::DeleteAccount(_)))
            .map(|(r, _)| [r.x + 4.0, r.y + 4.0])
            .unwrap();
        assert!(ctrl.handle_mouse_press(del_pos, 0));
        assert!(ctrl.handle_mouse_press(del_pos, 0));
        assert_eq!(*bridge.name.lock().unwrap(), ("BootPlayer".into(), true));

        // On non-title screen (e.g. "options"), key_press does NOT double-type before handle_char_typed
        ctrl.render(&mut canvas, "options", [0.0, 0.0], "ru_ru");
        assert!(ctrl.handle_mouse_press(off_pos, 0));
        assert!(ctrl.handle_key_press("key.keyboard.a", 0, "options"));
        assert!(ctrl.offline_name_field.value.is_empty());
        assert!(ctrl.handle_char_typed('a'));
        assert_eq!(ctrl.offline_name_field.value, "a");
        assert!(ctrl.handle_key_press(KEY_ESCAPE, 0, "options"));

        // + Microsoft defaults to DeviceCode and works without manual Azure Client ID
        let ms_pos = ctrl
            .last_hit_regions
            .iter()
            .find(|(_, t)| matches!(t, ClickTarget::OpenAddMicrosoft))
            .map(|(r, _)| [r.x + 4.0, r.y + 4.0])
            .unwrap();
        assert!(ctrl.handle_mouse_press(ms_pos, 0));
        assert_eq!(
            ctrl.modal,
            ModalView::AddMicrosoft {
                method: OAuthLoginKind::DeviceCode
            }
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
