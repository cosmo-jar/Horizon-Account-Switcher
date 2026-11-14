// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_screen_char_typed(
    game: &crate::game::Game<'_>,
    c: char,
    canceled: &mut bool,
) {
    crate::game::account_switcher_glue::char_typed(game, c, canceled);
}
