struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = i32(vertex_index) / 2;
    let y = i32(vertex_index) & 1;
    out.tex_coords = vec2<f32>(f32(x), f32(y));
    out.position = vec4<f32>(
        f32(x) * 2.0 - 1.0,
        1.0 - f32(y) * 2.0,
        0.0, 1.0
    );
    return out;
}

@fragment
fn fs_flat(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(0);
}

@group(0)
@binding(0)
var r_color: texture_2d<f32>;
@group(0)
@binding(1)
var r_sampler: sampler;

@fragment
fn fs_blit(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(r_color, r_sampler, in.tex_coords);
}
