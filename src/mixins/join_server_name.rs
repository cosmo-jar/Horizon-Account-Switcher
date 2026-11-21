fn as_join_server_name(
    game: &mut Game<'_>,
    address: &str,
    name: &str,
    local: Option<horizon_protocol::connection::Pipe>,
    cookies: Option<horizon_client::net::Cookies>,
    lan: bool,
    realm: bool,
    op: impl FnOnce(&mut Game<'_>, &str, &str, Option<horizon_protocol::connection::Pipe>, Option<horizon_client::net::Cookies>, bool, bool) -> Result<()>,
) -> Result<()> {
    let resolved = crate::game::native_account_switcher::effective_join_name(game, name);
    op(game, address, &resolved, local, cookies, lan, realm)
}
