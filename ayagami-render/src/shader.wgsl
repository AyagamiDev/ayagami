// Vertex shader

struct GlobalUniform {
    view_mtx: mat4x4<f32>,
    srgb: u32,
}

struct ArtMeshUniform {
    // Note: Ordered to optimize packing & avoid padding
    multiply_color: vec3<f32>,
    opacity: f32,
    screen_color: vec3<f32>,
    mask_invert: u32,
}

@group(0) @binding(0)
var<uniform> u_global: GlobalUniform;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) mask_coords: vec2<f32>,
}

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.tex_coords = model.tex_coords;
    out.clip_position =
        u_global.view_mtx *
        vec4<f32>(model.position, 0.0, 1.0);
    let mask_x = (out.clip_position.x + 1) / 2;
    let mask_y = (1 - out.clip_position.y) / 2;
    out.mask_coords = vec2(mask_x, mask_y);
    return out;
}

// Fragment shader

@group(0) @binding(1)
var<uniform> u_artmesh: ArtMeshUniform;

@group(1) @binding(0)
var t_model: texture_2d<f32>;
@group(1) @binding(1)
var s_model: sampler;

@group(2) @binding(0)
var t_mask: texture_2d<f32>;
@group(2) @binding(1)
var s_mask: sampler;

// 0-1 sRGB gamma  from  0-1 linear
fn gamma_from_linear_rgb(rgb: vec3<f32>) -> vec3<f32> {
    let cutoff = rgb < vec3<f32>(0.0031308);
    let lower = rgb * vec3<f32>(12.92);
    let higher = vec3<f32>(1.055) * pow(rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(higher, lower, cutoff);
}

// 0-1 sRGBA gamma  from  0-1 linear
fn gamma_from_linear_rgba(linear_rgba: vec4<f32>) -> vec4<f32> {
    var a = saturate(linear_rgba.a);
    if linear_rgba.a <= 0 {
        return vec4<f32>(0.);
    }
    return vec4<f32>(
        linear_rgba.a * gamma_from_linear_rgb(linear_rgba.rgb / linear_rgba.a),
        linear_rgba.a
    );
}

fn artmesh_color(tex_coords: vec2<f32>) -> vec4<f32> {
    var p = textureSample(t_model, s_model, tex_coords);
    var rgb = p.rgb;
    rgb *= u_artmesh.multiply_color;
    rgb += u_artmesh.screen_color * p.a;
    if (u_global.srgb != 0) {
        return gamma_from_linear_rgba(saturate(vec4(rgb, p.a)));
    } else {
        return saturate(vec4(rgb, p.a));
    }
}

fn mask_value(pos: vec2<f32>) -> f32 {
    var m = textureSample(t_mask, s_mask, pos).r;
    if u_artmesh.mask_invert != 0 {
        return 1 - m;
    } else {
        return m;
    }
}

@fragment
fn fs_normal(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    p *= u_artmesh.opacity;
    return p;
}

@fragment
fn fs_multiply(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    p *= u_artmesh.opacity;
    p.r += 1 - p.a;
    p.g += 1 - p.a;
    p.b += 1 - p.a;
    return p;
}

@fragment
fn fs_normal_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    var m = mask_value(in.mask_coords);
    p *= u_artmesh.opacity * m;
    return p;
}

@fragment
fn fs_multiply_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    var m = mask_value(in.mask_coords);
    p *= u_artmesh.opacity * m;
    p.r += 1 - p.a;
    p.g += 1 - p.a;
    p.b += 1 - p.a;
    return p;
}

@fragment
fn fs_render_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = textureSample(t_model, s_model, in.tex_coords);
    return vec4<f32>(p.a);
}
