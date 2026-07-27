//! GLSL shader sources for the WebGL2 code-graph renderer, as Rust `const
//! &str` (zero JavaScript — shaders are compiled in Rust via `web-sys`).
//!
//! Lifted verbatim from the proven spike
//! (`docs/superpowers/spike-source/spike-c-webgl/code_graph_gl.rs`). The
//! attribute/uniform contract below is what `renderer.rs` wires up:
//!
//! - Node program: `aCorner` (loc 0, quad corner in [-1,1]²),
//!   `aPosRadius` (loc 1, per-instance x/y/radius), `aState` (loc 2,
//!   per-instance alpha/glow/colorMix/unused).
//! - Edge program: `aLocal` (loc 0, [0,1]×[-1,1] along/across local space),
//!   `aEndpoints` (loc 1, per-instance fromX/fromY/toX/toY), `aState`
//!   (loc 2, per-instance alpha/colorMix/unused/unused).
//! - Both share the `uResolution`/`uPan`/`uZoom` camera uniforms and the
//!   `uColorBase`/`uColorAccent` palette uniforms; edges add `uHalfWidth`.
//!
//! Both fragment shaders anti-alias with `fwidth` and `discard` below an
//! alpha floor; the node shader additionally renders the selection glow ring
//! outside r=1 (the quad is padded by `aState.y` in the vertex shader).

/// Node vertex shader: one quad instance per node, positioned by
/// `aPosRadius`, scaled by the glow pad, projected through the pan/zoom
/// camera into clip space.
pub const NODE_VS: &str = r#"#version 300 es
layout(location=0) in vec2 aCorner;
layout(location=1) in vec3 aPosRadius; // x, y, radius
layout(location=2) in vec4 aState;     // alpha, glow, colorMix, unused
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
    vState = aState;
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
/// `uHalfWidth` around the from→to segment in world space, then projected
/// through the same camera as the nodes.
pub const EDGE_VS: &str = r#"#version 300 es
layout(location=0) in vec2 aLocal; // (0/1 along length, -1/1 across width)
layout(location=1) in vec4 aEndpoints; // fromX, fromY, toX, toY
layout(location=2) in vec4 aState; // alpha, colorMix, unused, unused
uniform vec2 uResolution;
uniform vec2 uPan;
uniform float uZoom;
uniform float uHalfWidth;
out vec2 vLocal;
out vec4 vState;
void main() {
    vec2 from = aEndpoints.xy;
    vec2 to = aEndpoints.zw;
    vec2 dir = to - from;
    float len = max(length(dir), 0.0001);
    vec2 unit = dir / len;
    vec2 perp = vec2(-unit.y, unit.x);
    vec2 world = from + unit * len * aLocal.x + perp * uHalfWidth * aLocal.y;
    vec2 screen = world * uZoom + uPan;
    vec2 clip = vec2(screen.x / uResolution.x * 2.0 - 1.0, 1.0 - screen.y / uResolution.y * 2.0);
    gl_Position = vec4(clip, 0.0, 1.0);
    vLocal = aLocal;
    vState = aState;
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
