// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_screen_render(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client_render::hud::HudBatch,
    screen: &crate::mod_api::NativeScreenView<'_>,
) {
    crate::game::account_switcher_glue::render_screen(game, batch, screen);
}
