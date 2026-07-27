//! WebGL2 instanced renderer: context creation, program/shader compilation,
//! buffer setup, and the two-draw-call frame — ONE instanced draw call for
//! all edges, ONE for all nodes (`drawArraysInstanced`, native WebGL2, no
//! ANGLE extension needed).
//!
//! Lifted unchanged in behaviour from the proven spike
//! (`docs/superpowers/spike-source/spike-c-webgl/code_graph_gl.rs`):
//!
//! - [`Renderer::new`]            ← `setup_gl`
//! - [`Renderer::upload_static`]  ← `upload_static` (STATIC geometry buffers
//!   plus the `buffer_data` allocation of the DYNAMIC state buffers)
//! - [`Renderer::upload_dynamic`] ← `upload_dynamic` / `upload_sub` (in-place
//!   `buffer_sub_data` rewrite)
//! - [`Renderer::draw`]           ← `redraw`, minus the spike's one-shot
//!   bring-up debug log
//!
//! The spike's `GraphData`/`ViewState` inputs are replaced by plain slices so
//! this module stays pure GL plumbing: computing per-instance state from
//! graph/filter/animation facts, owning the camera, and driving the render
//! loop are the view layer's job (Plan C Task 4).
//!
//! Buffer strategy (from the spike): node/edge geometry is a shared 4-vertex
//! quad; per-instance data lives in two buffers per primitive kind:
//!   - a STATIC buffer (position/radius, or endpoints) written once per
//!     (re)layout via `buffer_data`
//!   - a DYNAMIC buffer (alpha/glow/color-mix) rewritten on every selection,
//!     filter, or animation-frame change via `buffer_sub_data` — no draw-call
//!     growth, just a data upload, so the "one draw call" constraint holds
//!     regardless of how often the view layer mutates state.
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlVertexArrayObject};

use super::shaders::{EDGE_FS, EDGE_VS, NODE_FS, NODE_VS};

// Colors, matching the styles.css design tokens (the same palette the SVG
// code-graph spike used, so the renderers are visually comparable).
const COLOR_CANVAS: [f32; 3] = [0x08 as f32 / 255.0, 0x09 as f32 / 255.0, 0x0b as f32 / 255.0];
const COLOR_NODE: [f32; 3] = [0x7a as f32 / 255.0, 0x86 as f32 / 255.0, 0x99 as f32 / 255.0];
const COLOR_EDGE: [f32; 3] = [0x3b as f32 / 255.0, 0x49 as f32 / 255.0, 0x4b as f32 / 255.0];
const COLOR_ACCENT: [f32; 3] = [0x19 as f32 / 255.0, 0xf2 as f32 / 255.0, 0xc4 as f32 / 255.0];

const EDGE_HALF_WIDTH: f32 = 0.55; // world units

fn compile_shader(gl: &Gl, kind: u32, src: &str) -> Result<web_sys::WebGlShader, String> {
    let shader = gl.create_shader(kind).ok_or("create_shader failed")?;
    gl.shader_source(&shader, src);
    gl.compile_shader(&shader);
    if gl.get_shader_parameter(&shader, Gl::COMPILE_STATUS).as_bool().unwrap_or(false) {
        Ok(shader)
    } else {
        Err(gl.get_shader_info_log(&shader).unwrap_or_else(|| "unknown shader error".into()))
    }
}

fn link_program(gl: &Gl, vs_src: &str, fs_src: &str) -> Result<WebGlProgram, String> {
    let vs = compile_shader(gl, Gl::VERTEX_SHADER, vs_src)?;
    let fs = compile_shader(gl, Gl::FRAGMENT_SHADER, fs_src)?;
    let program = gl.create_program().ok_or("create_program failed")?;
    gl.attach_shader(&program, &vs);
    gl.attach_shader(&program, &fs);
    gl.link_program(&program);
    if gl.get_program_parameter(&program, Gl::LINK_STATUS).as_bool().unwrap_or(false) {
        Ok(program)
    } else {
        Err(gl.get_program_info_log(&program).unwrap_or_else(|| "unknown link error".into()))
    }
}

fn make_buffer(gl: &Gl, data: &[f32], usage: u32) -> WebGlBuffer {
    let buf = gl.create_buffer().expect("create_buffer");
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buf));
    unsafe {
        let view = js_sys::Float32Array::view(data);
        gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, usage);
    }
    buf
}

fn upload_sub(gl: &Gl, buf: &WebGlBuffer, data: &[f32]) {
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(buf));
    unsafe {
        let view = js_sys::Float32Array::view(data);
        gl.buffer_sub_data_with_i32_and_array_buffer_view(Gl::ARRAY_BUFFER, 0, &view);
    }
}

/// GPU-side renderer state: the WebGL2 context, the two compiled programs,
/// the two VAOs, and the four per-instance buffers. Kept out of Leptos
/// signals by the caller — a redraw is imperative GPU submission work, not a
/// vdom diff.
pub struct Renderer {
    gl: Gl,
    node_program: WebGlProgram,
    edge_program: WebGlProgram,
    node_vao: WebGlVertexArrayObject,
    edge_vao: WebGlVertexArrayObject,
    // STATIC per-instance data (position/radius, endpoints): rewritten with
    // `buffer_data` (size may change) whenever the graph (re)simulates.
    node_pos_buf: WebGlBuffer,
    edge_endpoints_buf: WebGlBuffer,
    // DYNAMIC per-instance data (alpha/glow/color-mix): rewritten with
    // `buffer_sub_data` (same size, in place) on every selection/filter/
    // animation-frame change — this is the "no new draw call" path.
    node_state_buf: WebGlBuffer,
    edge_state_buf: WebGlBuffer,
    // Instance counts from the last `upload_static` (0 until then, so an
    // early `draw` clears the canvas and draws nothing).
    node_count: i32,
    edge_count: i32,
}

impl Renderer {
    /// Create the WebGL2 context for `canvas`, compile/link both programs,
    /// and build the VAOs with their shared quad geometry. Fails with a
    /// human-readable message when WebGL2 is unavailable or the shaders do
    /// not compile — the view layer renders an explicit error surface for
    /// this, never a blank canvas.
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, String> {
        let ctx = canvas
            .get_context("webgl2")
            .map_err(|_| "get_context threw".to_string())?
            .ok_or("webgl2 unsupported (get_context returned null)")?;
        let gl: Gl = ctx.unchecked_into();

        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);

        let node_program = link_program(&gl, NODE_VS, NODE_FS)?;
        let edge_program = link_program(&gl, EDGE_VS, EDGE_FS)?;

        // Shared quad geometry: nodes use [-1,1]x[-1,1] (circle local space);
        // edges use [0,1]x[-1,1] (length x width local space).
        let node_corners: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        let edge_corners: [f32; 8] = [0.0, -1.0, 1.0, -1.0, 0.0, 1.0, 1.0, 1.0];

        let node_vao = gl.create_vertex_array().ok_or("create_vertex_array (node)")?;
        gl.bind_vertex_array(Some(&node_vao));
        let node_corner_buf = make_buffer(&gl, &node_corners, Gl::STATIC_DRAW);
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&node_corner_buf));
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
        gl.enable_vertex_attrib_array(0);
        // Instance buffers are (re)bound with real data once the graph is
        // built (`upload_static`); allocate the VAO's attrib slots now so the
        // pointers are already wired to *some* buffer object.
        let node_pos_buf = gl.create_buffer().ok_or("create_buffer (node pos)")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&node_pos_buf));
        gl.vertex_attrib_pointer_with_i32(1, 3, Gl::FLOAT, false, 0, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_divisor(1, 1);
        let node_state_buf = gl.create_buffer().ok_or("create_buffer (node state)")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&node_state_buf));
        gl.vertex_attrib_pointer_with_i32(2, 4, Gl::FLOAT, false, 0, 0);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_divisor(2, 1);

        let edge_vao = gl.create_vertex_array().ok_or("create_vertex_array (edge)")?;
        gl.bind_vertex_array(Some(&edge_vao));
        let edge_corner_buf = make_buffer(&gl, &edge_corners, Gl::STATIC_DRAW);
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&edge_corner_buf));
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
        gl.enable_vertex_attrib_array(0);
        let edge_endpoints_buf = gl.create_buffer().ok_or("create_buffer (edge endpoints)")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&edge_endpoints_buf));
        gl.vertex_attrib_pointer_with_i32(1, 4, Gl::FLOAT, false, 0, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_divisor(1, 1);
        let edge_state_buf = gl.create_buffer().ok_or("create_buffer (edge state)")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&edge_state_buf));
        gl.vertex_attrib_pointer_with_i32(2, 4, Gl::FLOAT, false, 0, 0);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_divisor(2, 1);

        gl.bind_vertex_array(None);

        Ok(Self {
            gl,
            node_program,
            edge_program,
            node_vao,
            edge_vao,
            node_pos_buf,
            edge_endpoints_buf,
            node_state_buf,
            edge_state_buf,
            node_count: 0,
            edge_count: 0,
        })
    }

    /// Rebuild the STATIC per-instance buffers for a freshly (re)laid-out
    /// graph, and (re)allocate the DYNAMIC state buffers to match. Uses
    /// `buffer_data` (not `buffer_sub_data`) since the instance count may
    /// change between layouts.
    ///
    /// - `node_pos_radius`: interleaved `[x, y, radius] * node_count`
    /// - `edge_endpoints`: interleaved `[from_x, from_y, to_x, to_y] * edge_count`
    ///
    /// MUST be called before [`Renderer::upload_dynamic`] whenever the
    /// instance counts change: the trailing zero-fill below gives the dynamic
    /// state buffers their backing store via `buffer_data` (allocates), and
    /// `upload_dynamic`'s `buffer_sub_data` requires the buffer to already
    /// have storage of at least that size. Calling `buffer_sub_data` on a
    /// buffer that was only ever `create_buffer`'d (zero-sized) is a WebGL
    /// `INVALID_VALUE` error that silently no-ops the write — which is
    /// exactly what left every node/edge alpha at 0 (invisible) the first
    /// time the spike ran. Initial content is irrelevant: the view layer's
    /// first `upload_dynamic` overwrites it before the first paint.
    pub fn upload_static(&mut self, node_pos_radius: &[f32], edge_endpoints: &[f32]) {
        let gl = &self.gl;
        self.node_count = (node_pos_radius.len() / 3) as i32;
        self.edge_count = (edge_endpoints.len() / 4) as i32;

        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.node_pos_buf));
        unsafe {
            let view = js_sys::Float32Array::view(node_pos_radius);
            gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, Gl::STATIC_DRAW);
        }

        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.edge_endpoints_buf));
        unsafe {
            let view = js_sys::Float32Array::view(edge_endpoints);
            gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, Gl::STATIC_DRAW);
        }

        // See the doc comment: `buffer_data` first, then `buffer_sub_data`
        // is legal for the rest of this layout's lifetime.
        let node_state_zeros = vec![0.0_f32; self.node_count as usize * 4];
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.node_state_buf));
        unsafe {
            let view = js_sys::Float32Array::view(&node_state_zeros);
            gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, Gl::DYNAMIC_DRAW);
        }
        let edge_state_zeros = vec![0.0_f32; self.edge_count as usize * 4];
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.edge_state_buf));
        unsafe {
            let view = js_sys::Float32Array::view(&edge_state_zeros);
            gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &view, Gl::DYNAMIC_DRAW);
        }
    }

    /// Rewrite the DYNAMIC per-instance state buffers in place — the path
    /// every click/filter/animation-frame takes. Same size every time
    /// (node/edge count is fixed between `upload_static` calls), so
    /// `buffer_sub_data` never reallocates.
    ///
    /// - `node_state`: interleaved `[alpha, glow, color_mix, 0.0] * node_count`
    /// - `edge_state`: interleaved `[alpha, color_mix, 0.0, 0.0] * edge_count`
    pub fn upload_dynamic(&self, node_state: &[f32], edge_state: &[f32]) {
        upload_sub(&self.gl, &self.node_state_buf, node_state);
        upload_sub(&self.gl, &self.edge_state_buf, edge_state);
    }

    /// Draw one frame: clear to the canvas color, then edges (under the
    /// nodes) and nodes — each in ONE instanced draw call, same painter's
    /// order as the canvas2d spike. `pan`/`zoom` are the camera in CSS-pixel
    /// screen space, matching the shader contract (`screen = world * zoom +
    /// pan`); the viewport and `uResolution` come from the canvas's intrinsic
    /// (backing-store) size, which the caller keeps in sync with its CSS size
    /// and device pixel ratio.
    pub fn draw(&self, canvas: &HtmlCanvasElement, pan_x: f32, pan_y: f32, zoom: f32) {
        let gl = &self.gl;
        gl.viewport(0, 0, canvas.width() as i32, canvas.height() as i32);
        gl.clear_color(COLOR_CANVAS[0], COLOR_CANVAS[1], COLOR_CANVAS[2], 1.0);
        gl.clear(Gl::COLOR_BUFFER_BIT);
        let (w, h) = (canvas.width() as f32, canvas.height() as f32);

        gl.use_program(Some(&self.edge_program));
        gl.bind_vertex_array(Some(&self.edge_vao));
        let loc = gl.get_uniform_location(&self.edge_program, "uResolution");
        gl.uniform2f(loc.as_ref(), w, h);
        let loc = gl.get_uniform_location(&self.edge_program, "uPan");
        gl.uniform2f(loc.as_ref(), pan_x, pan_y);
        let loc = gl.get_uniform_location(&self.edge_program, "uZoom");
        gl.uniform1f(loc.as_ref(), zoom);
        let loc = gl.get_uniform_location(&self.edge_program, "uHalfWidth");
        gl.uniform1f(loc.as_ref(), EDGE_HALF_WIDTH);
        let loc = gl.get_uniform_location(&self.edge_program, "uColorBase");
        gl.uniform3f(loc.as_ref(), COLOR_EDGE[0], COLOR_EDGE[1], COLOR_EDGE[2]);
        let loc = gl.get_uniform_location(&self.edge_program, "uColorAccent");
        gl.uniform3f(loc.as_ref(), COLOR_ACCENT[0], COLOR_ACCENT[1], COLOR_ACCENT[2]);
        if self.edge_count > 0 {
            gl.draw_arrays_instanced(Gl::TRIANGLE_STRIP, 0, 4, self.edge_count);
        }

        gl.use_program(Some(&self.node_program));
        gl.bind_vertex_array(Some(&self.node_vao));
        let loc = gl.get_uniform_location(&self.node_program, "uResolution");
        gl.uniform2f(loc.as_ref(), w, h);
        let loc = gl.get_uniform_location(&self.node_program, "uPan");
        gl.uniform2f(loc.as_ref(), pan_x, pan_y);
        let loc = gl.get_uniform_location(&self.node_program, "uZoom");
        gl.uniform1f(loc.as_ref(), zoom);
        let loc = gl.get_uniform_location(&self.node_program, "uColorBase");
        gl.uniform3f(loc.as_ref(), COLOR_NODE[0], COLOR_NODE[1], COLOR_NODE[2]);
        let loc = gl.get_uniform_location(&self.node_program, "uColorAccent");
        gl.uniform3f(loc.as_ref(), COLOR_ACCENT[0], COLOR_ACCENT[1], COLOR_ACCENT[2]);
        if self.node_count > 0 {
            gl.draw_arrays_instanced(Gl::TRIANGLE_STRIP, 0, 4, self.node_count);
        }
    }
}
