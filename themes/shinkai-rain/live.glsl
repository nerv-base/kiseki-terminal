// ============================================================================
// ★ Kiseki Terminal — Shinkai Rain Live Cinematic Shader for Ghostty (60fps GPU)
// Parallax diagonal Tokyo night rain streaks, wet puddle raindrop rings,
// and warm konbini/streetlamp bokeh breathing — code text stays razor sharp.
// ============================================================================

const vec3 BASE_BG = vec3(0.0471, 0.0941, 0.1373); // #0C1823

float hash21(vec2 p) {
    p = fract(p * vec2(234.34, 435.345));
    p += dot(p, p + 34.23);
    return fract(p.x * p.y);
}

float bgLayerMask(vec4 term, vec2 uv) {
    vec3 unpremul = term.rgb / max(term.a, 0.001);
    float diff = length(unpremul - BASE_BG);
    float luma = dot(unpremul, vec3(0.299, 0.587, 0.114));
    float codeZone = 1.0 - smoothstep(0.52, 0.72, uv.x);
    float glyph = smoothstep(0.22, 0.48, luma) * smoothstep(0.18, 0.42, diff);
    return 1.0 - glyph * mix(0.45, 0.88, codeZone);
}

float RainLayer(vec2 uv, float cols, float speed, float seed) {
    vec2 ruv = vec2(uv.x + uv.y * 0.22, uv.y);
    ruv.x *= cols;
    float colId  = floor(ruv.x);
    float xLocal = fract(ruv.x) - 0.5;

    float h = hash21(vec2(colId, seed));
    float dropSpeed = speed * (0.80 + 0.45 * h);
    float yScroll   = ruv.y * 2.4 - iTime * dropSpeed + h * 15.0;
    float rowId     = floor(yScroll);
    float yLocal    = fract(yScroll);

    float h2 = hash21(vec2(colId, rowId + seed * 31.7));
    if (h2 < 0.52) return 0.0;

    float xOff    = (h2 - 0.76) * 0.55;
    float streakX = smoothstep(0.055, 0.0, abs(xLocal - xOff));
    float streakY = smoothstep(0.0, 0.65, yLocal) * smoothstep(1.0, 0.84, yLocal);
    return streakX * streakY * (0.45 + 0.55 * h);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res  = max(iResolution.xy, vec2(1.0));
    vec2 uv   = fragCoord / res;
    vec4 term = texture(iChannel0, uv);
    vec3 col  = term.rgb / max(term.a, 0.001);

    float bgMask = bgLayerMask(term, uv);

    // 1. Subtle wet pavement shimmer strictly in bottom-right walkway (artwork only)
    float puddleMask = smoothstep(0.76, 0.86, uv.x) * smoothstep(0.72, 0.82, uv.y);
    if (puddleMask > 0.01 && bgMask > 0.6) {
        float rip1 = sin(uv.y * 140.0 - iTime * 3.2 + uv.x * 35.0);
        float rip2 = cos(uv.x * 95.0  + iTime * 2.4 - uv.y * 60.0);
        vec2 puddleOff = vec2(rip1 * 0.0012, rip2 * 0.0008) * puddleMask * bgMask;
        vec4 sPuddle = texture(iChannel0, clamp(uv + puddleOff, 0.0, 1.0));
        col = mix(col, sPuddle.rgb / max(sPuddle.a, 0.001), puddleMask * bgMask * 0.75);
    }

    // 2. Soft warm streetlamp & turquoise konbini bokeh breathing glow
    float luma      = dot(col, vec3(0.299, 0.587, 0.114));
    float bokehMask = smoothstep(0.32, 0.75, luma) * smoothstep(0.48, 0.66, uv.x) * bgMask;
    float breathe   = 0.5 + 0.5 * sin(iTime * 1.35 + uv.x * 3.0);
    col += col * bokehMask * (0.14 * breathe);

    // 3. Dual-layer parallax falling Tokyo night rain streaks
    vec2 aspectUV = vec2(uv.x * (res.x / res.y), uv.y);
    float rainFar  = RainLayer(aspectUV, 42.0, 1.35, 1.0);
    float rainNear = RainLayer(aspectUV, 26.0, 2.05, 7.3);

    vec3 aquaRain  = vec3(0.45, 0.86, 0.92);
    vec3 warmTint  = vec3(1.00, 0.82, 0.52);
    float rightWarm = smoothstep(0.55, 0.90, uv.x) * 0.35;
    vec3 rainCol   = mix(aquaRain, warmTint, rightWarm);
    col += rainCol * (rainFar * 0.12 + rainNear * 0.20) * bgMask;

    // 4. Tiny concentric puddle raindrop ring highlights in wet walkway
    if (puddleMask > 0.05) {
        vec2 pCell = floor(aspectUV * 10.0);
        vec2 pLoc  = fract(aspectUV * 10.0) - 0.5;
        float ph   = hash21(pCell + 19.1);
        float pt   = fract(iTime * 0.55 + ph);
        float rad  = pt * 0.42;
        float dist = length(vec2(pLoc.x, pLoc.y * 2.2));
        float ring = exp(-abs(dist - rad) * 45.0) * (1.0 - pt) * smoothstep(0.0, 0.1, pt);
        col += vec3(0.36, 0.90, 0.82) * ring * puddleMask * 0.22 * bgMask;
    }

    vec3 outRgb = clamp(col, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
