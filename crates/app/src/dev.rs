use std::sync::OnceLock;

static ISOLATED: OnceLock<bool> = OnceLock::new();

pub fn isolated() -> bool {
    return *ISOLATED.get_or_init(|| std::env::var_os("LW_UI_NO_DESKTOP").is_some_and(|value| !value.is_empty()));
}
