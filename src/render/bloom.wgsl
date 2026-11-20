@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var original: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
struct Parameters { values: vec4<f32>, tint: vec4<f32> }
@group(0) @binding(3) var<uniform> params: Parameters;

struct Output { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Output {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Output(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
@fragment fn threshold(in: Output) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let a = textureSample(source, linear_sampler, in.uv + texel * vec2<f32>(-1.0, -1.0)).rgb;
    let b = textureSample(source, linear_sampler, in.uv + texel * vec2<f32>(1.0, -1.0)).rgb;
    let c = textureSample(source, linear_sampler, in.uv + texel * vec2<f32>(-1.0, 1.0)).rgb;
    let d = textureSample(source, linear_sampler, in.uv + texel * vec2<f32>(1.0, 1.0)).rgb;
    var color = (a + b + c + d) * 0.25;
    color *= clamp(max(max(color.r, color.g), color.b) - params.values.x, 0.0, 1.0);
    let gray = dot(color, vec3<f32>(0.2989, 0.5870, 0.1140));
    return vec4<f32>(max(vec3<f32>(0.0), (2.0 * color - gray) * params.values.y * params.tint.rgb), 1.0);
}
@fragment fn blur(in: Output) -> @location(0) vec4<f32> {
    let weights = array<f32, 7>(0.171834, 0.156756, 0.119007, 0.075189, 0.039533, 0.017298, 0.006299);
    let step = params.values.xy / vec2<f32>(textureDimensions(source));
    var color = textureSample(source, linear_sampler, in.uv).rgb * weights[0];
    for (var i = 1u; i < 7u; i++) {
        color += (textureSample(source, linear_sampler, in.uv + f32(i) * step).rgb + textureSample(source, linear_sampler, in.uv - f32(i) * step).rgb) * weights[i];
    }
    return vec4<f32>(color, 1.0);
}
@fragment fn combine(in: Output) -> @location(0) vec4<f32> {
    let base = textureSample(original, linear_sampler, in.uv);
    return vec4<f32>(base.rgb + textureSample(source, linear_sampler, in.uv).rgb, base.a);
}
