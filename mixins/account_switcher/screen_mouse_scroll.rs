// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_screen_mouse_scroll(
    game: &crate::game::Game<'_>,
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    crate::game::account_switcher_glue::mouse_scroll(game, scroll, canceled);
}
