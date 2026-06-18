//! Data layer: serde models for the Architext documents and same-origin fetch.
pub mod fetch;
pub mod live;
pub mod models;

pub use fetch::{
    fetch_cli_version, fetch_farm_plan, fetch_repo_tree, load_architecture_data, ArchitectureData,
    FetchError,
};
