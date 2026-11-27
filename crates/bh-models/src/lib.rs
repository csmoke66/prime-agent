//! The model catalog compiled into the client.
//! Everything the catalog can do is select among transports the client
//! compiled in; it can never introduce a transport or change what a
//! request sends. Depends on bh-ai and bh-types; nothing above bh-agent.

pub mod catalog;
pub mod compat;
pub mod pinning;
pub mod schema;
pub mod transports;

mod chain;

pub use chain::ModelCatalog;
pub use bh_types::ai::Model;
