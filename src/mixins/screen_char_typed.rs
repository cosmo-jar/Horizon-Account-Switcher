fn as_screen_char_typed(game: &Game<'_>, c: char, canceled: &mut bool) {
    crate::game::native_account_switcher::char_typed(game, c, canceled);
}
