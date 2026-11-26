// ============================================================================
// Kiseki Terminal — Starlight 8-Bit Live Pixel Shader (Ghostty postprocess)
//
// Real-time pixel-quantized twinkling stars, periodic diagonal shooting stars,
// shimmering pixel ocean reflection, and cozy CRT/moonlight pulse.
//
// Parameterized template: kiseki-shader injects KISEKI_* macros and shared
// helpers (`kiseki_bg_mask`, `kiseki_narrow_factor`).
// ============================================================================

float kiseki_hash21(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

vec3 kiseki_unpremul(vec4 texel) {
    return texel.rgb / max(texel.a, 0.001);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv = fragCoord / res;
    vec4 term = texture(iChannel0, uv);
    vec3 col = kiseki_unpremul(term);

    float bgMask = kiseki_bg_mask(term, uv);
    float narrow = kiseki_narrow_factor(res);
    float scale = max(1.0, round(min(res.x, res.y) / 720.0));

    // --- 1. Subtle pixel-stepped ocean wave shimmer -------------------------
    vec3 oceanShimmer = vec3(0.0);
#if KISEKI_QUALITY_LEVEL >= 2
    float oceanMask = smoothstep(0.54, 0.61, uv.y)
        * (1.0 - smoothstep(0.76, 0.85, uv.y))
        * smoothstep(0.32, 0.48, uv.x)
        * (1.0 - smoothstep(0.76, 0.86, uv.x))
        * narrow;
    float pixRow = floor(fragCoord.y / (3.0 * scale));
    float wavePhase = sin(pixRow * 0.65 + iTime * KISEKI_PARAM_WAVE_SPEED);
    oceanShimmer = KISEKI_ACCENT_PRIMARY * max(0.0, wavePhase) * 0.055 * oceanMask * bgMask;
#endif

    // --- 2. Cozy breathing pulse on glowing cyan CRT monitor & moon ---------
    float rightZone = smoothstep(0.58, 0.78, uv.x) * narrow;
    float cyanGlow = clamp(col.g * 0.6 + col.b * 0.6 - col.r * 0.5, 0.0, 1.0);
    float crtPulse = 0.5 + 0.5 * sin(iTime * KISEKI_PARAM_CRT_PULSE_HZ);
    vec3 glowAdd = col * cyanGlow * rightZone * (0.14 * crtPulse) * bgMask;

    // --- 3. Crisp 3x3 pixel-grid twinkling stars in upper midnight sky ------
    float pixSize = max(2.0, round(3.0 * scale));
    vec2 pixCoord = floor(fragCoord / max(pixSize, 1.0));
    vec2 cell = floor(pixCoord / 18.0);
    vec2 local = pixCoord - cell * 18.0;
    float rnd = kiseki_hash21(cell);

    vec3 starAdd = vec3(0.0);
    float skyMask = (1.0 - smoothstep(0.42, 0.58, uv.y)) * (1.0 - smoothstep(0.58, 0.78, uv.x));
    if (rnd > 0.74 && skyMask > 0.01) {
        vec2 starPos = floor(vec2(kiseki_hash21(cell + 17.3), kiseki_hash21(cell + 53.7)) * 14.0) + 2.0;
        float d = max(abs(local.x - starPos.x), abs(local.y - starPos.y));
        if (d < 0.6) {
            float twinkle = pow(0.5 + 0.5 * sin(iTime * (1.4 + rnd * 2.6) + rnd * 40.0), 3.0);
            vec3 starCol = (rnd > 0.92) ? KISEKI_ACCENT_SECONDARY :
                           (rnd > 0.83) ? KISEKI_ACCENT_PRIMARY :
                                          vec3(0.95, 0.97, 1.0);
            starAdd = starCol * twinkle * 0.45 * skyMask * bgMask;
        }
    }

    // --- 4. Periodic 8-bit diagonal pixel shooting star ---------------------
    vec3 meteorAdd = vec3(0.0);
#if KISEKI_QUALITY_LEVEL >= 2
    float cycle = max(KISEKI_PARAM_METEOR_CYCLE_SEC, 1.0);
    float idx = floor(iTime / cycle);
    float tLocal = fract(iTime / cycle);
    if (tLocal < 0.24) {
        float prog = tLocal / 0.24;
        vec2 startP = vec2(
            0.10 + kiseki_hash21(vec2(idx, 3.1)) * 0.24,
            0.08 + kiseki_hash21(vec2(idx, 9.7)) * 0.16
        );
        vec2 dir = normalize(vec2(0.86, 0.51));
        vec2 head = startP + dir * (prog * 0.34);

        vec2 qUV = (floor(fragCoord / max(pixSize, 1.0)) + 0.5) * pixSize / res;
        vec2 rel = qUV - head;
        float along = dot(rel, -dir);
        float perp = abs(dot(rel, vec2(-dir.y, dir.x)));
        if (along >= 0.0 && along < 0.085 && perp < (pixSize * 0.75 / max(res.y, 1.0))) {
            float tailFrac = along / 0.085;
            float fade = (1.0 - tailFrac) * sin(prog * 3.14159265);
            vec3 meteorCol = mix(vec3(0.98, 0.99, 1.0), KISEKI_ACCENT_PRIMARY, tailFrac);
            meteorAdd = meteorCol * fade * 0.82 * (1.0 - smoothstep(0.55, 0.70, uv.x)) * bgMask;
        }
    }
#endif

    vec3 outCol = col + oceanShimmer + glowAdd + starAdd + meteorAdd;

    // --- 5. Optional retro CRT phosphor scanlines & gentle vignette ---------
#if KISEKI_ENABLE_CRT
    float scanline = 0.88 + 0.12 * (0.5 + 0.5 * sin(fragCoord.y * 0.78539816));
    vec2 vigUv = uv * 2.0 - 1.0;
    float vignette = 1.0 - 0.18 * dot(vigUv, vigUv) * 0.5;
    outCol *= mix(1.0, scanline * vignette, 0.55 + 0.45 * bgMask);
#endif

    vec3 outRgb = clamp(outCol, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
