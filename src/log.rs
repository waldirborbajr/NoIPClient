use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) static DEBUG: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_verbosity(level: u8) {
    DEBUG.store(level >= 2, Ordering::Relaxed);
}

pub(crate) fn log_info(msg: &str) {
    println!("{msg}");
}
pub(crate) fn log_warn(msg: &str) {
    eprintln!("[warn] {msg}");
}
pub(crate) fn log_error(msg: &str) {
    eprintln!("[error] {msg}");
}
pub(crate) fn log_debug(msg: &str) {
    if DEBUG.load(Ordering::Relaxed) {
        eprintln!("[debug] {msg}");
    }
}
