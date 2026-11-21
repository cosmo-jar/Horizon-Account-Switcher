fn as_screen_render(
    game: &Game<'_>,
    batch: &mut HudBatch,
    screen: &ScreenView<'_>,
) {
    crate::game::native_account_switcher::render_screen(game, batch, screen);
}
