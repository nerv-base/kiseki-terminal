// ============================================================================
// Kiseki Terminal — Shinkai Rain Live Cinematic Shader (Ghostty postprocess)
//
// Parallax diagonal Tokyo night rain streaks, wet puddle raindrop rings,
// and warm konbini/streetlamp bokeh breathing — code text stays razor sharp.
//
// Parameterized template: kiseki-shader injects KISEKI_* macros and shared
// helpers (`kiseki_bg_mask`, `kiseki_narrow_factor`).
// ============================================================================

float kiseki_hash21(vec2 p) {
    p = fract(p * vec2(234.34, 435.345));
    p += dot(p, p + 34.23);
    return fract(p.x * p.y);
}

vec3 kiseki_unpremul(vec4 texel) {
    return texel.rgb / max(texel.a, 0.001);
}

float kiseki_rain_layer(vec2 uv, float cols, float speed, float seed) {
    vec2 ruv = vec2(uv.x + uv.y * 0.22, uv.y);
    ruv.x *= cols;
    float colId = floor(ruv.x);
    float xLocal = fract(ruv.x) - 0.5;

    float h = kiseki_hash21(vec2(colId, seed));
    float dropSpeed = speed * (0.80 + 0.45 * h);
    float yScroll = ruv.y * 2.4 - iTime * dropSpeed + h * 15.0;
    float rowId = floor(yScroll);
    float yLocal = fract(yScroll);

    float h2 = kiseki_hash21(vec2(colId, rowId + seed * 31.7));
    if (h2 < 0.52) {
        return 0.0;
    }

    float xOff = (h2 - 0.76) * 0.55;
    float streakX = 1.0 - smoothstep(0.0, 0.055, abs(xLocal - xOff));
    float streakY = smoothstep(0.0, 0.65, yLocal) * (1.0 - smoothstep(0.84, 1.0, yLocal));
    return streakX * streakY * (0.45 + 0.55 * h);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv = fragCoord / res;
    vec4 term = texture(iChannel0, uv);
    vec3 col = kiseki_unpremul(term);

    float bgMask = kiseki_bg_mask(term, uv);
    float narrow = kiseki_narrow_factor(res);
    float aspect = res.x / max(res.y, 1.0);

    // --- 1. Subtle wet pavement shimmer strictly in bottom-right walkway ----
    float puddleMask = smoothstep(0.76, 0.86, uv.x) * smoothstep(0.72, 0.82, uv.y) * narrow;
#if KISEKI_QUALITY_LEVEL >= 2
    if (puddleMask > 0.01 && bgMask > 0.6) {
        float rip1 = sin(uv.y * 140.0 - iTime * 3.2 + uv.x * 35.0);
        float rip2 = cos(uv.x * 95.0 + iTime * 2.4 - uv.y * 60.0);
        vec2 puddleOff = vec2(rip1 * 0.0012, rip2 * 0.0008) * puddleMask * bgMask;
        vec4 sPuddle = texture(iChannel0, clamp(uv + puddleOff, 0.0, 1.0));
        col = mix(col, kiseki_unpremul(sPuddle), puddleMask * bgMask * 0.75);
    }
#endif

    // --- 2. Soft warm streetlamp & turquoise konbini bokeh breathing glow ---
    float luma = dot(col, vec3(0.299, 0.587, 0.114));
    float bokehMask = smoothstep(0.32, 0.75, luma) * smoothstep(0.48, 0.66, uv.x) * bgMask * narrow;
    float breathe = 0.5 + 0.5 * sin(iTime * KISEKI_PARAM_BOKEH_PULSE_HZ + uv.x * 3.0);
    col += col * bokehMask * (0.14 * breathe);

    // --- 3. Parallax falling Tokyo night rain streaks -----------------------
    vec2 aspectUV = vec2(uv.x * aspect, uv.y);
    float speed = KISEKI_PARAM_RAIN_SPEED;
    float rainFar = kiseki_rain_layer(aspectUV, 42.0, 1.35 * speed, 1.0);

    vec3 aquaRain = mix(KISEKI_ACCENT_PRIMARY, vec3(0.55, 0.88, 0.96), 0.35);
    float rightWarm = smoothstep(0.55, 0.90, uv.x) * 0.35;
    vec3 rainCol = mix(aquaRain, KISEKI_ACCENT_SECONDARY, rightWarm);
    col += rainCol * (rainFar * 0.14) * bgMask;

#if KISEKI_QUALITY_LEVEL >= 2
    float rainNear = kiseki_rain_layer(aspectUV, 26.0, 2.05 * speed, 7.3);
    col += rainCol * (rainNear * 0.20) * bgMask;

    // --- 4. Concentric puddle raindrop ring highlights in wet walkway -------
    if (puddleMask > 0.05) {
        vec2 pCell = floor(aspectUV * 10.0);
        vec2 pLoc = fract(aspectUV * 10.0) - 0.5;
        float ph = kiseki_hash21(pCell + 19.1);
        float pt = fract(iTime * KISEKI_PARAM_PUDDLE_RING_RATE + ph);
        float rad = pt * 0.42;
        float dist = length(vec2(pLoc.x, pLoc.y * 2.2));
        float ring = exp(-abs(dist - rad) * 45.0) * (1.0 - pt) * smoothstep(0.0, 0.1, pt);
        col += KISEKI_ACCENT_PRIMARY * ring * puddleMask * 0.22 * bgMask;
    }
#endif

    vec3 outRgb = clamp(col, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
