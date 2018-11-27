use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use std::fmt::Debug;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Enables profiling.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Profiles the duration of a named section of code passed in as a closure.
///
/// When profiling isn't enabled, this has very low overhead.
pub fn profile<E, F, R>(name: &str, extra: E, f: F) -> R
where F: FnOnce() -> R, E: Debug {
    if ENABLED.load(Ordering::Acquire) {
        let start = Instant::now();
        let result = f();
        let duration = Instant::now() - start;
        info!("{}.{:03}s {} {:?}", duration.as_secs(), duration.subsec_millis(), name, extra);
        result
    } else {
        f()
    }
}
