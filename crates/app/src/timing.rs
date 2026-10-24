//! Debug-level timing logs for chasing load latency: `RUST_LOG=notebook_app=debug`.

use std::fmt::Display;

use std::time::Instant;

pub fn timed<T>(label: impl Display, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    log::debug!("{label}: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    result
}
