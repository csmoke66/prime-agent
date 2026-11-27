//! Private-frame codec re-export. The codec is the shared worker-socket wire
//! contract (served by this crate's workers, spoken by direct-attach clients
//! in bh-tui/bh-cli), so it lives in [`bh_types::daemon::framing`].

pub use bh_types::daemon::framing::*;
