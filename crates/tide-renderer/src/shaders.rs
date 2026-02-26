// WGSL shader source code for the rect and glyph pipelines.

pub const RECT_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

struct Uniforms {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Convert pixel coords to NDC: x: [0, width] -> [-1, 1], y: [0, height] -> [1, -1]
    let ndc_x = (in.position.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (in.position.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

pub const CHROME_RECT_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) rect_center: vec2<f32>,
    @location(3) rect_half: vec2<f32>,
    @location(4) corner_radius: f32,
    @location(5) shadow_blur: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) pixel_pos: vec2<f32>,
    @location(2) rect_center: vec2<f32>,
    @location(3) rect_half: vec2<f32>,
    @location(4) corner_radius: f32,
    @location(5) shadow_blur: f32,
};

struct Uniforms {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = (in.position.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (in.position.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = in.color;
    out.pixel_pos = in.position;
    out.rect_center = in.rect_center;
    out.rect_half = in.rect_half;
    out.corner_radius = in.corner_radius;
    out.shadow_blur = in.shadow_blur;
    return out;
}

fn sdf_rounded_rect(p: vec2<f32>, center: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let cr = min(r, min(half.x, half.y));
    let d = abs(p - center) - half + vec2(cr);
    return length(max(d, vec2(0.0))) + min(max(d.x, d.y), 0.0) - cr;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = sdf_rounded_rect(in.pixel_pos, in.rect_center, in.rect_half, in.corner_radius);
    if in.shadow_blur > 0.0 {
        // Shadow mode: soft gaussian-like falloff over blur radius
        let alpha = 1.0 - smoothstep(-in.shadow_blur * 0.5, in.shadow_blur, dist);
        if alpha < 0.001 { discard; }
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    } else {
        // Normal mode: crisp SDF edge
        let alpha = 1.0 - smoothstep(-1.0, 0.5, dist);
        if alpha < 0.001 { discard; }
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    }
}
"#;

// ── MSDF helper: shared by all glyph fragment shaders ──

const MSDF_FRAGMENT_COMMON: &str = "
// MSDF distance range in texels (must match generation parameter)
const MSDF_PX_RANGE: f32 = 4.0;

fn median3(r: f32, g: f32, b: f32) -> f32 {
    return max(min(r, g), min(max(r, g), b));
}

fn msdf_alpha(uv: vec2<f32>) -> f32 {
    let msd = textureSample(atlas_texture, atlas_sampler, uv);
    let sd = median3(msd.r, msd.g, msd.b);

    // Compute screen-space pixel range from texture derivatives
    let unit_range = vec2<f32>(MSDF_PX_RANGE) / vec2<f32>(textureDimensions(atlas_texture));
    let screen_tex_size = 1.0 / fwidth(uv);
    let screen_px_range = max(0.5 * dot(unit_range, screen_tex_size), 1.0);

    let screen_px_distance = screen_px_range * (sd - 0.5);
    return clamp(screen_px_distance + 0.5, 0.0, 1.0);
}
";

// ── Instanced grid shaders ──
// GPU generates quad corners from vertex_index; one instance = one cell.

pub const GRID_BG_INSTANCED_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

struct Uniforms {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) inst_pos: vec2<f32>,
    @location(1) inst_size: vec2<f32>,
    @location(2) inst_color: vec4<f32>,
) -> VertexOutput {
    // Generate quad corner from vertex_index (two triangles: 0,1,2 + 3,4,5)
    let x = select(0.0, 1.0, vi == 1u || vi == 2u || vi == 4u);
    let y = select(0.0, 1.0, vi == 2u || vi == 4u || vi == 5u);

    let pos = inst_pos + vec2(x * inst_size.x, y * inst_size.y);

    var out: VertexOutput;
    let ndc_x = (pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = inst_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

// Grid glyph instanced shader is built by concatenating MSDF common + specific code.
pub fn grid_glyph_instanced_shader() -> String {
    format!(
        r#"
struct VertexOutput {{
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}};

struct Uniforms {{
    screen_size: vec2<f32>,
}};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) inst_pos: vec2<f32>,
    @location(1) inst_size: vec2<f32>,
    @location(2) inst_uv_min: vec2<f32>,
    @location(3) inst_uv_max: vec2<f32>,
    @location(4) inst_color: vec4<f32>,
) -> VertexOutput {{
    let x = select(0.0, 1.0, vi == 1u || vi == 2u || vi == 4u);
    let y = select(0.0, 1.0, vi == 2u || vi == 4u || vi == 5u);

    let pos = inst_pos + vec2(x * inst_size.x, y * inst_size.y);
    let uv = mix(inst_uv_min, inst_uv_max, vec2(x, y));

    var out: VertexOutput;
    let ndc_x = (pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    out.color = inst_color;
    return out;
}}

{msdf_common}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {{
    let alpha = msdf_alpha(in.uv);
    if alpha < 0.001 {{ discard; }}
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}}
"#,
        msdf_common = MSDF_FRAGMENT_COMMON,
    )
}

// Overlay/chrome glyph shader (indexed, non-instanced) with MSDF.
pub fn glyph_shader() -> String {
    format!(
        r#"
struct VertexInput {{
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}};

struct VertexOutput {{
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}};

struct Uniforms {{
    screen_size: vec2<f32>,
}};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {{
    var out: VertexOutput;
    let ndc_x = (in.position.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (in.position.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}}

{msdf_common}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {{
    let alpha = msdf_alpha(in.uv);
    if alpha < 0.001 {{ discard; }}
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}}
"#,
        msdf_common = MSDF_FRAGMENT_COMMON,
    )
}
