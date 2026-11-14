// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_settings_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    crate::game::account_switcher_glue::settings_click(game, mod_id, button);
}
