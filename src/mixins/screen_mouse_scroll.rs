fn as_screen_mouse_scroll(game: &Game<'_>, scroll: [f64; 2], canceled: &mut bool) {
    crate::game::native_account_switcher::mouse_scroll(game, scroll, canceled);
}
