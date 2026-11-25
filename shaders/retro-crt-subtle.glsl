// ============================================================================
// ★ Kiseki Terminal — Subtle Retro CRT Phosphor & Scanline Shader for Ghostty
// Preserves razor-sharp pixel typography while adding cozy CRT atmosphere.
// ============================================================================

vec3 sampleUnpremul(vec2 uv) {
    vec4 s = texture(iChannel0, clamp(uv, 0.0, 1.0));
    return s.rgb / max(s.a, 0.001);
}

vec3 SampleBloom(vec2 uv, vec2 texel) {
    vec3 sum = vec3(0.0);
    sum += sampleUnpremul(uv + vec2(-texel.x,  0.0))     * 0.20;
    sum += sampleUnpremul(uv + vec2( texel.x,  0.0))     * 0.20;
    sum += sampleUnpremul(uv + vec2( 0.0,     -texel.y)) * 0.20;
    sum += sampleUnpremul(uv + vec2( 0.0,      texel.y)) * 0.20;
    sum += sampleUnpremul(uv + vec2(-texel.x, -texel.y)) * 0.05;
    sum += sampleUnpremul(uv + vec2( texel.x, -texel.y)) * 0.05;
    sum += sampleUnpremul(uv + vec2(-texel.x,  texel.y)) * 0.05;
    sum += sampleUnpremul(uv + vec2( texel.x,  texel.y)) * 0.05;
    return sum;
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res   = max(iResolution.xy, vec2(1.0));
    vec2 uv    = fragCoord / res;
    vec2 texel = 1.0 / res;

    vec4 term = texture(iChannel0, uv);
    vec3 col  = term.rgb / max(term.a, 0.001);

    // 1. Gentle phosphor bloom only on bright foreground glyphs
    vec3 blur      = SampleBloom(uv, texel * 1.25);
    float luma     = dot(blur, vec3(0.299, 0.587, 0.114));
    float glowMask = smoothstep(0.32, 0.85, luma);
    col += blur * glowMask * 0.14;

    // 2. Whisper-soft 2px CRT scanlines (6% modulation - never hurts legibility)
    float scan = sin(fragCoord.y * 3.14159265) * 0.5 + 0.5;
    col *= mix(0.94, 1.0, scan);

    // 3. Very subtle edge vignette for cozy CRT depth
    vec2 centered = uv * 2.0 - 1.0;
    float vig = 1.0 - dot(centered, centered) * 0.06;
    col *= clamp(vig, 0.0, 1.0);

    vec3 outRgb = clamp(col, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
