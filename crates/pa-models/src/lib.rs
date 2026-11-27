//! The model catalog compiled into the client.
//! Everything the catalog can do is select among transports the client
//! compiled in; it can never introduce a transport or change what a
//! request sends. Depends on pa-ai and pa-types; nothing above pa-agent.

pub mod catalog;
pub mod compat;
pub mod pinning;
pub mod schema;
pub mod transports;

mod chain;

pub use chain::ModelCatalog;
pub use pa_types::ai::Model;
