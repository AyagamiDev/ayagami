// Vertex shader

struct GlobalUniform {
    view_mtx: mat4x4<f32>,
}

struct ArtMeshUniform {
    // Note: Ordered to optimize packing & avoid padding
    multiply_color: vec3<f32>,
    opacity: f32,
    screen_color: vec3<f32>,
    mask_invert: u32,
    linear_to_srgb: u32,
    color_blend: u32,
    alpha_blend: u32,
    _pad: u32,
}

const A_Over = 0;
const A_Atop = 1;
const A_Out = 2;
const A_Conjoint = 3;
const A_Disjoint = 4;

const C_Normal = 0;
const C_Add = 3;
const C_AddGlow = 4;
const C_Darken = 5;
const C_Multiply = 6;
const C_ColorBurn = 7;
const C_LinearBurn = 8;
const C_Lighten = 9;
const C_Screen = 10;
const C_ColorDodge = 11;
const C_Overlay = 12;
const C_SoftLight = 13;
const C_HardLight = 14;
const C_LinearLight = 15;
const C_Hue = 16;
const C_Color = 17;

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

@vertex
fn vs_blit(
    @builtin(vertex_index) vertex_index: u32
) -> VertexOutput {
    var out: VertexOutput;
    let x = (i32(vertex_index) / 2) * 2;
    let y = (i32(vertex_index) & 1) * 2;
    out.tex_coords = vec2<f32>(f32(x), f32(y));
    out.mask_coords = out.tex_coords;
    out.clip_position = vec4<f32>(
        f32(x) * 2.0 - 1.0,
        1.0 - f32(y) * 2.0,
        0.0, 1.0
    );
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

@group(3) @binding(0)
var t_fb: texture_2d<f32>;
@group(3) @binding(1)
var s_fb: sampler;

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
    if (u_artmesh.linear_to_srgb != 0) {
        p = gamma_from_linear_rgba(p);
    }
    var a = p.a;
    var rgb = p.rgb;
    rgb *= u_artmesh.multiply_color;
    rgb = (a - (a - rgb) * (1 - u_artmesh.screen_color));
    return saturate(vec4(rgb, p.a));
}

fn mask_value(pos: vec2<f32>) -> f32 {
    var m = textureSample(t_mask, s_mask, pos).r;
    if u_artmesh.mask_invert != 0 {
        return 1 - m;
    } else {
        return m;
    }
}

fn multiply(Cb: vec3<f32>, Cs: vec3<f32>) -> vec3<f32> {
    return Cs * Cb;
}

fn screen(Cb: vec3<f32>, Cs: vec3<f32>) -> vec3<f32> {
    return Cb + Cs - (Cb * Cs);
}

fn hard_light(Cb: vec3<f32>, Cs: vec3<f32>) -> vec3<f32> {
    return select(screen(Cb, 2 * Cs - 1), multiply(Cb, 2 * Cs), Cs <= vec3(0.5));
}

fn soft_light_d(Cb: vec3<f32>) -> vec3<f32> {
    return select(sqrt(Cb), ((16 * Cb - 12) * Cb + 4) * Cb, Cb <= vec3(0.25));
}

fn lum(C: vec3<f32>) -> f32 {
    return 0.3 * C.r + 0.59 * C.g + 0.11 * C.b;
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    var C = c;
    let L = lum(C);
    let n = min(min(C.r, C.g), C.b);
    let x = max(max(C.r, C.g), C.b);

    if (n < 0) {
        C = L + (((C - L) * L) / (L - n));
    }

    if (x > 1) {
        C = L + (((C - L) * (1 - L)) / (x - L));
    }

    return C;
}

fn set_lum(C: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum(C);
    return clip_color(C + d);
}

fn sat(C: vec3<f32>) -> f32 {
    let ma = max(max(C.r, C.g), C.b);
    let mi = min(min(C.r, C.g), C.b);

    return ma - mi;
}

fn set_sat_x(c: vec3<f32>, s: f32) -> vec3<f32> {
    var C = c;
    if C.r > C.b {
        C.g = (((C.g - C.b) * s) / (C.r - C.b));
        C.r = s;
    } else {
        C.g = 0;
        C.r = 0;
    }
    C.b = 0;
    return C;
}

fn set_sat(C: vec3<f32>, s: f32) -> vec3<f32> {
    if (C.r >= C.g) && (C.g >= C.b) {
        return set_sat_x(C, s);
    } else if (C.r >= C.b) && (C.b >= C.g) {
        return set_sat_x(C.rbg, s).rbg;
    } else if (C.b >= C.r) && (C.r >= C.g) {
        return set_sat_x(C.brg, s).gbr;
    } else if (C.b >= C.r) && (C.g >= C.r) {
        return set_sat_x(C.bgr, s).bgr;
    } else if (C.g >= C.r) && (C.r >= C.b) {
        return set_sat_x(C.grb, s).grb;
    } else {
        return set_sat_x(C.gbr, s).brg;
    }
}

fn advanced_blend(pos: vec2<f32>, sample: vec4<f32>, opacity: f32) -> vec4<f32> {
    let src = sample * opacity;
    let dst = textureSample(t_fb, s_fb, pos);

    let As = src.a;
    let Ab = dst.a;
    let Cs = select(vec3(0), src.rgb / As, As > 0);
    let Cb = select(vec3(0), dst.rgb / Ab, Ab > 0);

    var Cm: vec3<f32>;

    switch (u_artmesh.color_blend) {
        default { // C_Normal
            Cm = Cs;
        }
        case C_Add {
            Cm = saturate(Cb + Cs);
        }
        case C_AddGlow {
            Cm = Cb + Cs;
        }
        case C_Darken {
            Cm = min(Cb, Cs);
        }
        case C_Multiply {
            Cm = multiply(Cb, Cs);
        }
        case C_ColorBurn {
            Cm = select(1 - saturate((1 - Cb) / Cs), vec3(0), Cs == vec3(0));
        }
        case C_LinearBurn {
            Cm = saturate(Cb + Cs - 1);
        }
        case C_Lighten {
            Cm = min(Cb, Cs);
        }
        case C_Screen {
            Cm = screen(Cb, Cs);
        }
        case C_ColorDodge {
            Cm = select(saturate(Cb / (1 - Cs)), vec3(1), Cs == vec3(1));
        }
        case C_Overlay {
            Cm = hard_light(Cs, Cb);
        }
        case C_SoftLight {
            Cm = select(
                Cb + (2 * Cs - 1) * (soft_light_d(Cb) - Cb),
                Cb - (1 - 2 * Cs) * Cb * (1 - Cb),
                Cs <= vec3(0.5)
            );
        }
        case C_HardLight {
            Cm = select(screen(Cb, 2 * Cs - 1), multiply(Cb, 2 * Cs), Cs <= vec3(0.5));
        }
        case C_LinearLight {
            Cm = saturate(Cb + 2 * Cs - 1);
        }
        case C_Hue {
            Cm = set_lum(set_sat(Cs, sat(Cb)), lum(Cb));
        }
        case C_Color {
            Cm = set_lum(Cs, lum(Cb));
        }
    }

    var Cr = (1 - Ab) * Cs + Ab * Cm;

    var co: vec3<f32>;
    var ao: f32;

    switch (u_artmesh.alpha_blend) {
        default { // A_Over
            co = As * Cr + Ab * Cb * (1 - As);
            ao = As + Ab * (1 - As);
        }
        case A_Atop {
            co = As * Cr * Ab + Ab * Cb * (1 - As);
            ao = Ab; // As * Ab + Ab * (1 - As);
        }
        case A_Out { // Destination out
            ao = Ab * (1 - As);
            co = ao * Cb;
        }
        case A_Conjoint {
            co = As * Cr + saturate(Ab - As) * Cb;
            ao = max(As, Ab);
        }
        case A_Disjoint {
            co = As * Cr + min(Ab, 1 - As) * Cb;
            ao = saturate(As + Ab);
        }
    }

    return vec4(co, ao);
}

@fragment
fn fs_normal(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    p *= u_artmesh.opacity;
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
fn fs_render_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = textureSample(t_model, s_model, in.tex_coords);
    return vec4<f32>(p.a);
}

@fragment
fn fs_advanced(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    return advanced_blend(in.mask_coords, p, u_artmesh.opacity);
}

@fragment
fn fs_advanced_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = artmesh_color(in.tex_coords);
    var m = mask_value(in.mask_coords);
    return advanced_blend(in.mask_coords, p, u_artmesh.opacity * m);
}
