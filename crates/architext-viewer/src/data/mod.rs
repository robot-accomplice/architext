//! Data layer: serde models for the Architext documents and same-origin fetch.
pub mod fetch;
pub mod models;

pub use fetch::{load_architecture_data, ArchitectureData, FetchError};
