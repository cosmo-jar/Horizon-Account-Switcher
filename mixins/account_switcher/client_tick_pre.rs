// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_client_tick_pre(game: &mut crate::game::Game<'_>) {
    crate::game::account_switcher_glue::sync_options(game);
}
