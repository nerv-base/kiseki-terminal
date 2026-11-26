// ============================================================================
// Kiseki Terminal — Kyoto Sakura Live Fantasy Shader (Ghostty postprocess)
//
// Drifting & tumbling sakura petals, meandering golden shrine fireflies,
// warm flickering stone lantern glow, and tranquil moonlit pond shimmer.
//
// Parameterized template: kiseki-shader injects KISEKI_* macros and shared
// helpers (`kiseki_bg_mask`, `kiseki_narrow_factor`).
// ============================================================================

float kiseki_hash21(vec2 p) {
    p = fract(p * vec2(127.1, 311.7));
    p += dot(p, p + 34.5);
    return fract(p.x * p.y);
}

vec3 kiseki_unpremul(vec4 texel) {
    return texel.rgb / max(texel.a, 0.001);
}

float kiseki_sakura_layer(vec2 aspectUV, float gridScale, float speed, float seed) {
    vec2 p = aspectUV * gridScale;
    p.x += iTime * speed * 0.55;
    p.y -= iTime * speed * 0.42;

    vec2 id = floor(p);
    vec2 f = fract(p) - 0.5;
    float h = kiseki_hash21(id + seed);
    if (h < 0.62) {
        return 0.0;
    }

    float phase = h * 31.4159 + iTime * (1.2 + h * 1.4);
    vec2 offset = vec2(sin(phase * 0.7) * 0.24, cos(phase * 0.9) * 0.20);
    vec2 q = f - offset;

    float ang = phase * 0.85;
    float ca = cos(ang);
    float sa = sin(ang);
    vec2 rq = vec2(q.x * ca - q.y * sa, q.x * sa + q.y * ca);
    float flutter = 0.45 + 0.55 * abs(sin(phase * 1.3));
    float d = length(vec2(rq.x * 1.2, rq.y * (2.8 / max(flutter, 0.001))));
    return (1.0 - smoothstep(0.032, 0.082, d)) * (0.55 + 0.45 * h);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv = fragCoord / res;
    vec4 term = texture(iChannel0, uv);
    vec3 col = kiseki_unpremul(term);

    float bgMask = kiseki_bg_mask(term, uv);
    float narrow = kiseki_narrow_factor(res);
    float aspect = res.x / max(res.y, 1.0);

    // --- 1. Tranquil moonlit temple pond shimmer in bottom-right reflection -
#if KISEKI_QUALITY_LEVEL >= 2
    float pondMask = smoothstep(0.50, 0.64, uv.x) * smoothstep(0.76, 0.86, uv.y) * narrow;
    if (pondMask > 0.01 && bgMask > 0.6) {
        float rippleX = sin(uv.y * 125.0 - iTime * 1.8 + uv.x * 18.0)
            * KISEKI_PARAM_POND_RIPPLE_AMP * pondMask * bgMask;
        float rippleY = cos(uv.x * 70.0 + iTime * 1.3)
            * (KISEKI_PARAM_POND_RIPPLE_AMP * 0.44) * pondMask * bgMask;
        vec4 sPond = texture(iChannel0, clamp(uv + vec2(rippleX, rippleY), 0.0, 1.0));
        col = mix(col, kiseki_unpremul(sPond), pondMask * bgMask * 0.75);
    }
#endif

    // --- 2. Warm organic stone lantern candle-glow flicker ------------------
    float lanternDist = length((uv - vec2(0.615, 0.44)) * vec2(1.6, 1.0));
    float lanternHalo = exp(-lanternDist * 6.5) * narrow;
    float flicker = 0.55
        + 0.28 * sin(iTime * KISEKI_PARAM_LANTERN_FLICKER_HZ)
        + 0.17 * sin(iTime * 6.1 + 1.4);
    col += KISEKI_ACCENT_SECONDARY * lanternHalo * (0.11 * flicker) * bgMask;

    // --- 3. Parallax layers of drifting & fluttering sakura petals ----------
    vec2 aspectUV = vec2(uv.x * aspect, uv.y);
    float speed = KISEKI_PARAM_PETAL_SPEED;
    float petalsFar = kiseki_sakura_layer(aspectUV, 5.2, 0.28 * speed, 3.7);
    col += KISEKI_ACCENT_PRIMARY * (petalsFar * 0.24) * bgMask;

#if KISEKI_QUALITY_LEVEL >= 2
    float petalsNear = kiseki_sakura_layer(aspectUV, 3.4, 0.42 * speed, 11.9);
    vec3 sakuraLight = mix(KISEKI_ACCENT_PRIMARY, vec3(1.0, 0.94, 0.97), 0.55);
    col += sakuraLight * (petalsNear * 0.32) * bgMask;
#endif

    // --- 4. Meandering golden shrine fireflies ------------------------------
#if KISEKI_QUALITY_LEVEL >= 2
    for (int i = 0; i < 6; i++) {
        float fi = float(i);
        vec2 center = vec2(
            0.22 + 0.52 * fract(sin(fi * 12.9898) * 43758.5453),
            0.24 + 0.56 * fract(sin(fi * 78.2330) * 43758.5453)
        );
        vec2 wander = vec2(
            sin(iTime * (0.28 + fi * 0.06) + fi * 2.1) * 0.075,
            cos(iTime * (0.22 + fi * 0.05) + fi * 1.7) * 0.055
        );
        vec2 fp = vec2((center.x + wander.x) * aspect, center.y + wander.y);
        float d = length(aspectUV - fp);
        float pulse = pow(0.5 + 0.5 * sin(iTime * (1.1 + fi * 0.25) + fi * 3.7), 2.5);
        float orb = exp(-d * 115.0) * 0.55 + exp(-d * 30.0) * 0.14;
        col += KISEKI_ACCENT_SECONDARY * orb * pulse * bgMask;
    }
#else
    for (int i = 0; i < 3; i++) {
        float fi = float(i);
        vec2 center = vec2(
            0.22 + 0.52 * fract(sin(fi * 12.9898) * 43758.5453),
            0.24 + 0.56 * fract(sin(fi * 78.2330) * 43758.5453)
        );
        vec2 wander = vec2(
            sin(iTime * (0.28 + fi * 0.06) + fi * 2.1) * 0.075,
            cos(iTime * (0.22 + fi * 0.05) + fi * 1.7) * 0.055
        );
        vec2 fp = vec2((center.x + wander.x) * aspect, center.y + wander.y);
        float d = length(aspectUV - fp);
        float pulse = pow(0.5 + 0.5 * sin(iTime * (1.1 + fi * 0.25) + fi * 3.7), 2.5);
        float orb = exp(-d * 115.0) * 0.55 + exp(-d * 30.0) * 0.14;
        col += KISEKI_ACCENT_SECONDARY * orb * pulse * bgMask;
    }
#endif

    vec3 outRgb = clamp(col, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
