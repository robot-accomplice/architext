//! WebGL2 code-graph renderer (Plan C): GLSL shader sources plus the
//! instanced-draw GL plumbing, lifted unchanged in behaviour from the proven
//! spike (`docs/superpowers/spike-source/spike-c-webgl/code_graph_gl.rs`).
//!
//! Zero JavaScript: shaders are GLSL `const &str` ([`shaders`]) compiled in
//! Rust via `web-sys`; all drawing goes through `WebGl2RenderingContext`
//! ([`renderer::Renderer`]).
//!
//! This module is deliberately view-free: it owns no Leptos signals, no
//! timers, and no render loop, so there is nothing here to cancel on
//! disposal. Pan/zoom, click hit-testing, chrome, and the animation/render
//! loop live in the view layer (Plan C Task 4), which must cancel any
//! loop it starts via Leptos `on_cleanup`.

pub mod renderer;
pub mod shaders;
