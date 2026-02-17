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
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) pixel_pos: vec2<f32>,
    @location(2) rect_center: vec2<f32>,
    @location(3) rect_half: vec2<f32>,
    @location(4) corner_radius: f32,
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
    let alpha = 1.0 - smoothstep(-1.0, 0.5, dist);
    if alpha < 0.001 {
        discard;
    }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

pub const GLYPH_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct Uniforms {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = (in.position.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (in.position.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;
