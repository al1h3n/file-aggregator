// Window background + final blit, for the glass layer (see glass.rs).
//
// fs_background draws the backdrop that the glass refracts into an sRGB texture:
//   mode 0  solid colour        (a = linear RGB)
//   mode 1  diagonal gradient   (a, b = OKLab stops; mixed in OKLab so the middle
//                                stays vivid instead of going muddy like plain RGB mixing)
//   mode 2  photo               (cover-fit, already downscaled on the CPU)
// fs_blit_* copies the finished glass texture into egui's render pass, pixel for pixel.

struct Background {
    a: vec4f,
    b: vec4f,
    // Photo cover fit: uv = pixel * photo.xy + photo.zw.
    photo: vec4f,
    // x: mode, yz: target size in pixels.
    info: vec4f,
};

@group(0) @binding(0) var<uniform> bg: Background;
@group(0) @binding(1) var photo_sampler: sampler;
@group(0) @binding(2) var photo_texture: texture_2d<f32>;

// Only the blit entry points use this (each pipeline's layout holds just what it uses).
@group(0) @binding(3) var glass_texture: texture_2d<f32>;

// One triangle that covers the whole target.
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
}

fn oklab_to_linear_srgb(c: vec3f) -> vec3f {
    let l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3f(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

fn srgb_encode(c: vec3f) -> vec3f {
    return select(1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3f(0.0031308));
}

fn srgb_decode(c: vec3f) -> vec3f {
    return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

// Interleaved gradient noise (Jimenez 2014): cheap per-pixel noise without a texture.
fn ign(p: vec2f) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2f(0.06711056, 0.00583715))));
}

@fragment
fn fs_background(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    let mode = u32(bg.info.x + 0.5);
    var color: vec3f;
    if mode == 2u {
        // The texture is sRGB, so sampling returns linear colour.
        color = textureSampleLevel(photo_texture, photo_sampler, pos.xy * bg.photo.xy + bg.photo.zw, 0.0).rgb;
    } else if mode == 1u {
        // 0 at the top-left corner, 1 at the bottom-right, eased so the ends are calm.
        let size = bg.info.yz;
        let t = smoothstep(0.0, 1.0, clamp(dot(pos.xy, size) / dot(size, size), 0.0, 1.0));
        color = max(oklab_to_linear_srgb(mix(bg.a.xyz, bg.b.xyz, t)), vec3f(0.0));
    } else {
        color = bg.a.xyz;
    }
    // Dither by half a step of the 8-bit sRGB output so gradients don't band.
    let encoded = srgb_encode(clamp(color, vec3f(0.0), vec3f(1.0))) + (ign(pos.xy) - 0.5) / 255.0;
    return vec4f(srgb_decode(clamp(encoded, vec3f(0.0), vec3f(1.0))), 1.0);
}

// egui renders into a non-sRGB surface (it blends in gamma space), so the linear values
// read from the sRGB glass texture are encoded back by hand.
@fragment
fn fs_blit_encode(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    return vec4f(srgb_encode(textureLoad(glass_texture, vec2i(pos.xy), 0).rgb), 1.0);
}

// For an sRGB surface the hardware encodes on write.
@fragment
fn fs_blit_linear(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    return vec4f(textureLoad(glass_texture, vec2i(pos.xy), 0).rgb, 1.0);
}
