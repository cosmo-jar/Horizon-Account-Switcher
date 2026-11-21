fn as_remote_auth(
    game: &Game<'_>,
    original: Option<std::sync::Arc<horizon_client::auth::AuthSession>>,
) -> Option<std::sync::Arc<horizon_client::auth::AuthSession>> {
    crate::game::native_account_switcher::effective_remote_auth(game, original)
}
