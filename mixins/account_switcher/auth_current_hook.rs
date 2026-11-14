// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_auth_current(
    original: Option<Arc<AuthSession>>,
) -> Option<Arc<AuthSession>> {
    crate::auth::account_switcher_override::override_session().unwrap_or(original)
}
