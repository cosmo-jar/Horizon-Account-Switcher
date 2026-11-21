fn as_screen_key_press(
    game: &Game<'_>,
    screen: &ScreenView<'_>,
    key: &str,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::native_account_switcher::key_press(game, screen, key, modifiers, canceled);
}
