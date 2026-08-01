//! GLSL shader sources for the WebGL2 code-graph renderer, as Rust `const
//! &str` (zero JavaScript — shaders are compiled in Rust via `web-sys`).
//!
//! Originally lifted from the proven spike
//! (`docs/superpowers/spike-source/spike-c-webgl/code_graph_gl.rs`); the edge
//! program's `aState.z` slot (was always-zero padding) was repurposed for the
//! progressive call-order animation (see `EDGE_VS`'s doc). The
//! attribute/uniform contract below is what `renderer.rs` wires up:
//!
//! - Node program: `aCorner` (loc 0, quad corner in [-1,1]²),
//!   `aPosRadius` (loc 1, per-instance x/y/radius), `aState` (loc 2,
//!   per-instance alpha/glow/colorMix/hopAge — `hopAge` drives the
//!   call-order animation's comet-trail brightness decay, computed on the
//!   CPU (`code_graph_view_model.rs`'s `hop_age`) and consumed here in
//!   `NODE_VS`; `0.0` outside an active, motion-enabled animation is the
//!   decay math's identity value, so every other path — selection,
//!   reduced-motion, animation off — is bit-for-bit unaffected).
//! - Edge program: `aLocal` (loc 0, [0,1]×[-1,1] along/across local space),
//!   `aEndpoints` (loc 1, per-instance fromX/fromY/toX/toY), `aState`
//!   (loc 2, per-instance alpha/colorMix/progress/hopAge — `progress`
//!   interpolates the drawn endpoint from `from` toward `to`, `1.0` meaning
//!   the full edge; `hopAge` decays brightness the same way the node
//!   program's does, see `EDGE_VS`'s doc).
//! - Both share the `uResolution`/`uPan`/`uZoom` camera uniforms and the
//!   `uColorBase`/`uColorAccent` palette uniforms; edges add `uHalfWidth`.
//!
//! Both fragment shaders anti-alias with `fwidth` and `discard` below an
//! alpha floor; the node shader additionally renders the selection glow ring
//! outside r=1 (the quad is padded by `aState.y` in the vertex shader).
//!
//! `aState.w`'s brightness decay is deliberately evaluated HERE, in the
//! vertex shader, not in Rust: `code_graph_view_model.rs`'s CPU loop already
//! re-uploads the whole dynamic buffer every animation frame (for
//! `progress`'s sake), so adding a per-instance AGE float to that existing
//! upload costs nothing extra asymptotically — but computing the actual
//! peak→floor brightness CURVE for 17,814 nodes / 50,215 edges on the CPU,
//! every frame, is exactly the cost this instanced-draw renderer exists to
//! avoid (see `gl/renderer.rs`'s doc). The GPU already evaluates this
//! shader once per vertex every frame regardless; folding the decay in is
//! free by comparison. `code_graph_view_model.rs`'s `decay_brightness` is
//! the unit-tested Rust twin of the identical formula below — keep the two
//! in sync by hand if either changes; a GLSL source string cannot import a
//! Rust `const`.

/// Node vertex shader: one quad instance per node, positioned by
/// `aPosRadius`, scaled by the glow pad, projected through the pan/zoom
/// camera into clip space.
pub const NODE_VS: &str = r#"#version 300 es
layout(location=0) in vec2 aCorner;
layout(location=1) in vec3 aPosRadius; // x, y, radius
layout(location=2) in vec4 aState;     // alpha, glow, colorMix, hopAge
uniform vec2 uResolution;
uniform vec2 uPan;
uniform float uZoom;
out vec2 vCorner;
out vec4 vState;
void main() {
    float pad = 1.0 + aState.y * 0.9;
    vec2 world = aPosRadius.xy + aCorner * aPosRadius.z * pad;
    vec2 screen = world * uZoom + uPan;
    vec2 clip = vec2(screen.x / uResolution.x * 2.0 - 1.0, 1.0 - screen.y / uResolution.y * 2.0);
    gl_Position = vec4(clip, 0.0, 1.0);
    vCorner = aCorner * pad;
    // Comet-trail brightness decay: aState.w ("hopAge") is hop-durations
    // since the BFS wavefront revealed this node, uploaded at its PEAK
    // alpha/colorMix (aState.x/z) regardless of age — the ramp down to the
    // resting floor happens right here via `mix`, not before upload (see
    // this file's module doc, and `code_graph_view_model.rs`'s `hop_age`/
    // `decay_brightness`, the canonical unit-tested formula this mirrors).
    // 3.0 = NODE_DECAY_HOPS hop-durations to fully decay; 0.35/0.6 are
    // TRAIL_ALPHA/NODE_MIX_FLOOR, the exact pre-decay resting values this
    // replaces. `age <= 0` clamps `t` to 0, making `mix(x, floor, 0.0) ==
    // x` the identity — the selection view and prefers-reduced-motion path
    // (both upload a constant `hopAge` of 0.0) are therefore unchanged.
    float t = clamp(aState.w / 3.0, 0.0, 1.0);
    vState = vec4(mix(aState.x, 0.35, t), aState.y, mix(aState.z, 0.6, t), 0.0);
}
"#;

/// Node fragment shader: anti-aliased filled circle inside r=1 (base color
/// mixed toward accent by `colorMix`), soft accent glow ring outside r=1
/// while `glow > 0`.
pub const NODE_FS: &str = r#"#version 300 es
precision highp float;
in vec2 vCorner;
in vec4 vState;
uniform vec3 uColorBase;
uniform vec3 uColorAccent;
out vec4 fragColor;
void main() {
    float r = length(vCorner);
    float alpha = vState.x;
    float glow = vState.y;
    vec3 color = mix(uColorBase, uColorAccent, vState.z);
    if (r <= 1.0) {
        float edge = fwidth(r) * 1.5 + 0.001;
        float a = smoothstep(1.0, 1.0 - edge, r) * alpha;
        if (a <= 0.003) discard;
        fragColor = vec4(color, a);
    } else if (glow > 0.001) {
        float span = max(glow * 0.9, 0.001);
        float t = (r - 1.0) / span;
        if (t > 1.0) discard;
        float a = (1.0 - t) * (1.0 - t) * glow * alpha * 0.65;
        if (a <= 0.003) discard;
        fragColor = vec4(uColorAccent, a);
    } else {
        discard;
    }
}
"#;

/// Edge vertex shader: one quad instance per directed edge, extruded to
/// `uHalfWidth` around the from→(progressive-)to segment in world space,
/// then projected through the same camera as the nodes.
///
/// Progressive edge-draw (call-order animation rework): `aState.z` is the
/// draw progress in `[0,1]` — the quad's far end is `mix(from, to,
/// progress)` instead of always `to`, so at `progress < 1.0` the line is
/// visibly SHORTER than the full edge, reading as still growing from `from`
/// toward `to`. `code_graph_view_model.rs`'s `cull` (with a `Wavefront`)
/// always orders `aEndpoints` lower-BFS-depth-endpoint-first, so `from` is
/// the already-reached end and `to` is the one hop ahead — the line grows in
/// the direction the wavefront is actually travelling, independent of the
/// call direction. `progress` is `1.0` for every edge outside an active
/// animation (unchanged full-length draw) and for any edge already fully
/// behind the wavefront.
pub const EDGE_VS: &str = r#"#version 300 es
layout(location=0) in vec2 aLocal; // (0/1 along length, -1/1 across width)
layout(location=1) in vec4 aEndpoints; // fromX, fromY, toX, toY
layout(location=2) in vec4 aState; // alpha, colorMix, progress, hopAge
uniform vec2 uResolution;
uniform vec2 uPan;
uniform float uZoom;
uniform float uHalfWidth;
out vec2 vLocal;
out vec4 vState;
void main() {
    vec2 from = aEndpoints.xy;
    vec2 to = mix(aEndpoints.xy, aEndpoints.zw, aState.z);
    vec2 dir = to - from;
    float len = max(length(dir), 0.0001);
    vec2 unit = dir / len;
    vec2 perp = vec2(-unit.y, unit.x);
    vec2 world = from + unit * len * aLocal.x + perp * uHalfWidth * aLocal.y;
    vec2 screen = world * uZoom + uPan;
    vec2 clip = vec2(screen.x / uResolution.x * 2.0 - 1.0, 1.0 - screen.y / uResolution.y * 2.0);
    gl_Position = vec4(clip, 0.0, 1.0);
    vLocal = aLocal;
    // Comet-trail brightness decay — same mechanism as `NODE_VS`, a faster
    // 1.5-hop span (EDGE_DECAY_HOPS) so the travelling line stays visually
    // distinct from the accumulating mass: edges encode DIRECTION via
    // `progress` above, and a slow decay would smear many hops' worth of
    // direction cues together. 0.22/0.5 are EDGE_ALPHA_FLOOR/EDGE_MIX_FLOOR,
    // the exact pre-decay resting alpha/colorMix this replaces. `age <= 0`
    // (not yet reached, or — negative — still mid-growth, see `hop_age`'s
    // doc) is the identity: unchanged for the growing edge, the Off path,
    // and prefers-reduced-motion.
    float t = clamp(aState.w / 1.5, 0.0, 1.0);
    vState = vec4(mix(aState.x, 0.22, t), mix(aState.y, 0.5, t), aState.z, 0.0);
}
"#;

/// Edge fragment shader: anti-aliased constant-width line across the quad's
/// width axis, base color mixed toward accent by `colorMix`.
pub const EDGE_FS: &str = r#"#version 300 es
precision highp float;
in vec2 vLocal;
in vec4 vState;
uniform vec3 uColorBase;
uniform vec3 uColorAccent;
out vec4 fragColor;
void main() {
    float d = abs(vLocal.y);
    float edge = fwidth(d) * 1.5 + 0.001;
    float a = (1.0 - smoothstep(1.0 - edge, 1.0, d)) * vState.x;
    if (a <= 0.003) discard;
    vec3 color = mix(uColorBase, uColorAccent, vState.y);
    fragColor = vec4(color, a);
}
"#;
