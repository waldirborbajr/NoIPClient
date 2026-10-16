use std::env;

/// True when running on Android (compile-time target) or inside Termux (runtime env).
pub(crate) fn is_termux() -> bool {
    cfg!(target_os = "android")
        || env::var_os("TERMUX_VERSION").is_some()
        || env::var("PREFIX").is_ok_and(|p| p.contains("com.termux"))
}
