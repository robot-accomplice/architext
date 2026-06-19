//! Data layer: serde models for the Architext documents and same-origin fetch.
pub mod fetch;
pub mod live;
pub mod models;
pub mod mutate;

pub use fetch::{
    fetch_cli_version, fetch_farm_plan, fetch_farm_plan_polling, fetch_file, fetch_node_git,
    fetch_repo_tree, load_architecture_data, ArchitectureData, FetchError,
};
pub use mutate::{fetch_mutation_token, post_mutation, MutationError};
