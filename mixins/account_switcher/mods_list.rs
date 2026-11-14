// Horizon Account Switcher — GPL-3.0
// Copyright (C) 2026 cosmo-jar

fn account_switcher_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    use crate::game::account_switcher_glue as m;
    mods.push(crate::game::native_hooks::NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: Vec::new(),
    });
}
