fn as_client_tick_pre(game: &mut Game<'_>) {
    crate::game::native_account_switcher::sync_options(game);
}
