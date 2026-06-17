//! Trunk CSR entry point. Mounts the Leptos app to <body>.
use architext_viewer::App;
use leptos::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
