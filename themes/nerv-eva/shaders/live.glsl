// ============================================================================
// Kiseki Terminal — NERV UNIT-02 Live Tactical Shader (Ghostty postprocess)
//
// Homage to nerv-base: sweeping MAGI amber scanline, AT-Field hex radar pulse,
// crimson blueprint heartbeat, and periodic tactical glitch accents.
//
// This is a *template*. kiseki-shader injects the KISEKI_* macro header and the
// shared helper library before compiling, so no color, opacity or geometry
// constant is ever hard-coded here.
//
// Constraints enforced by `kiseki theme validate`:
//   * every smoothstep(a, b, x) has a < b (no reversed edges)
//   * every texture division is guarded by max(alpha, 0.001)
//   * loop bounds are compile-time constants
//   * texture samples stay within the per-profile budget
// ============================================================================

// ---------------------------------------------------------------------------
// Helpers (the preprocessor also injects kiseki_bg_mask/kiseki_narrow_factor)
// ---------------------------------------------------------------------------

float kiseki_hash11(float p) {
    p = fract(p * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return fract(p);
}

// Hexagonal AT-Field cell edge distance, normalized to 0..1.
float kiseki_hex_edge(vec2 p) {
    p.x *= 1.1547005;
    float isTwo = fract(floor(p.x) * 0.5) * 2.0;
    p.y += isTwo * 0.5;
    vec2 f = abs(fract(p) - 0.5);
    float d = max(f.x * 1.5 + f.y * 0.866025, f.y * 1.73205);
    return smoothstep(0.81, 0.86, d);
}

// Un-premultiply a terminal sample without risking a divide-by-zero.
vec3 kiseki_unpremul(vec4 texel) {
    return texel.rgb / max(texel.a, 0.001);
}

// ---------------------------------------------------------------------------
// Entry point (wrapped by the generated main() shim)
// ---------------------------------------------------------------------------

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv = fragCoord / res;

    vec4 term = texture(iChannel0, uv);
    vec3 col = kiseki_unpremul(term);

    // Text-protection weight: 1.0 over pure background, -> 0.0 over glyphs.
    float bgMask = kiseki_bg_mask(term, uv);
    // Aspect-ratio fade: 0.0 in narrow split panes, 1.0 on wide windows.
    float narrow = kiseki_narrow_factor(res);

    float scale = max(1.0, round(min(res.x, res.y) / 720.0));

    // --- 1. Periodic micro RGB tactical glitch on the artwork zone only ------
#if KISEKI_QUALITY_LEVEL >= 2
    float glitchCycle = fract(iTime / KISEKI_PARAM_GLITCH_INTERVAL_SEC);
    float glitchOn = step(glitchCycle, 0.028);
    float stepTick = floor(iTime * 28.0);
    float bandA = step(0.28, uv.y) * step(uv.y, 0.34);
    float bandB = step(0.62, uv.y) * step(uv.y, 0.67);
    float rightOnly = smoothstep(0.56, 0.68, uv.x) * bgMask * narrow;
    float sliceMask = clamp(bandA + bandB, 0.0, 1.0) * glitchOn * rightOnly;

    if (sliceMask > 0.01) {
        float sliceShift = (kiseki_hash11(stepTick + bandA * 7.0) - 0.5) * 0.010 * sliceMask;
        float chromaOff = 0.0032 * sliceMask;
        vec2 uvBase = clamp(uv + vec2(sliceShift, 0.0), 0.0, 1.0);
        vec4 sR = texture(iChannel0, clamp(uvBase + vec2(-chromaOff, 0.0), 0.0, 1.0));
        vec4 sG = texture(iChannel0, uvBase);
        vec4 sB = texture(iChannel0, clamp(uvBase + vec2(chromaOff, 0.0), 0.0, 1.0));
        vec3 glitched = vec3(
            kiseki_unpremul(sR).r,
            kiseki_unpremul(sG).g,
            kiseki_unpremul(sB).b
        );
        col = mix(col, glitched, sliceMask);
    }
#endif

    // --- 2. Unit-02 blueprint wireframe breathing pulse ---------------------
    float artZone = smoothstep(0.50, 0.68, uv.x) * bgMask * narrow;
    float lineEnergy = clamp(
        max(col.r - KISEKI_BASE_BG.r, max(col.g - KISEKI_BASE_BG.g, col.b - KISEKI_BASE_BG.b)) * 2.2,
        0.0,
        1.0
    ) * artZone;
    float heartbeat = 0.5 + 0.5 * sin(iTime * KISEKI_PARAM_HEARTBEAT_HZ);
    col += col * lineEnergy * (0.18 * heartbeat);

    // --- 3. Slow sweeping MAGI amber tactical scan beam --------------------
    float scanY = fract(iTime / KISEKI_PARAM_SCAN_PERIOD_SEC);
    float dScan = abs(uv.y - scanY);
    float scanCore = exp(-dScan * 220.0);
    // NOTE: edges ordered low->high; the tail is expressed as a masked range.
    float scanTail = (1.0 - smoothstep(0.0, 0.08, scanY - uv.y)) * step(uv.y, scanY) * 0.25;

    col += KISEKI_ACCENT_PRIMARY * (scanCore * 0.15 + scanTail * 0.045) * bgMask;
    col += KISEKI_ACCENT_PRIMARY * lineEnergy * (scanCore * 0.60 + scanTail * 0.35);

    // --- 4. AT-Field hexagonal radar shimmer anchored on the art half ------
    vec2 hexCoord = fragCoord / (KISEKI_PARAM_HEX_CELL_PX * scale);
    float hexLine = kiseki_hex_edge(hexCoord);
    float hexZone = smoothstep(0.45, 0.82, uv.x)
        * (1.0 - smoothstep(0.0, 0.12, abs(uv.y - 0.5) - 0.38))
        * narrow;
    float hexWave = 0.5 + 0.5 * sin(iTime * 1.1 - uv.x * 4.0 + uv.y * 2.5);
    col += mix(KISEKI_ACCENT_SECONDARY, KISEKI_ACCENT_PRIMARY, hexWave)
        * hexLine * hexZone * (0.055 * hexWave) * bgMask;

    // --- 5. Whisper-soft tactical CRT micro-scanlines ----------------------
#if KISEKI_ENABLE_CRT
    float microScan = mix(1.0, 0.97 + 0.03 * (0.5 + 0.5 * sin(fragCoord.y * 3.14159265)), bgMask);
    col *= microScan;
#endif

    // --- 6. Porter-Duff Source-Over against the terminal's own alpha -------
    vec3 outRgb = clamp(col, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
