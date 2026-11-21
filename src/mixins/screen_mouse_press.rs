fn as_screen_mouse_press(
    game: &Game<'_>,
    screen: &ScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    crate::game::native_account_switcher::mouse_press(game, screen, button, canceled);
}
