// ============================================================================
// ★ Kiseki Terminal — Starlight 8-Bit Live Pixel Shader for Ghostty (60fps GPU)
// Real-time pixel-quantized twinkling stars, periodic diagonal shooting stars,
// shimmering pixel ocean reflection, and cozy CRT/moonlight pulse — sharp text.
// ============================================================================

const vec3 BASE_BG = vec3(0.0392, 0.0588, 0.1451); // #0A0F25

float hash21(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

// Protect bright foreground code glyphs so ambient overlays stay behind/around text
float bgLayerMask(vec4 term, vec2 uv) {
    vec3 unpremul = term.rgb / max(term.a, 0.001);
    float diff = length(unpremul - BASE_BG);
    float luma = dot(unpremul, vec3(0.299, 0.587, 0.114));
    // Left code reading zone protects bright text strongly; right artwork zone stays lively
    float codeZone = 1.0 - smoothstep(0.52, 0.72, uv.x);
    float glyph = smoothstep(0.22, 0.48, luma) * smoothstep(0.18, 0.42, diff);
    return 1.0 - glyph * mix(0.45, 0.88, codeZone);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv  = fragCoord / res;
    vec4 term = texture(iChannel0, uv);
    vec3 col  = term.rgb / max(term.a, 0.001);

    float bgMask = bgLayerMask(term, uv);
    float scale  = max(1.0, round(min(res.x, res.y) / 720.0));

    // 1. Subtle pixel-stepped ocean wave shimmer (mid-lower sea reflection band)
    float oceanMask = smoothstep(0.54, 0.61, uv.y) * (1.0 - smoothstep(0.76, 0.85, uv.y))
                    * smoothstep(0.32, 0.48, uv.x) * (1.0 - smoothstep(0.76, 0.86, uv.x));
    float pixRow = floor(fragCoord.y / (3.0 * scale));
    float wavePhase = sin(pixRow * 0.65 + iTime * 2.2);
    vec3 oceanShimmer = vec3(0.38, 0.88, 1.0) * max(0.0, wavePhase) * 0.055 * oceanMask * bgMask;

    // 2. Cozy breathing pulse on glowing cyan CRT monitor & mint moon highlights on right
    float rightZone = smoothstep(0.58, 0.78, uv.x);
    float cyanGlow  = clamp(col.g * 0.6 + col.b * 0.6 - col.r * 0.5, 0.0, 1.0);
    float crtPulse  = 0.5 + 0.5 * sin(iTime * 2.1);
    vec3 glowAdd    = col * cyanGlow * rightZone * (0.14 * crtPulse) * bgMask;

    // 3. Crisp 3x3 pixel-grid twinkling stars in upper midnight sky (left & mid sky)
    float pixSize = max(2.0, round(3.0 * scale));
    vec2 pixCoord = floor(fragCoord / pixSize);
    vec2 cell  = floor(pixCoord / 18.0);
    vec2 local = pixCoord - cell * 18.0;
    float rnd  = hash21(cell);

    vec3 starAdd = vec3(0.0);
    float skyMask = (1.0 - smoothstep(0.42, 0.58, uv.y)) * (1.0 - smoothstep(0.58, 0.78, uv.x));
    if (rnd > 0.74 && skyMask > 0.01) {
        vec2 starPos = floor(vec2(hash21(cell + 17.3), hash21(cell + 53.7)) * 14.0) + 2.0;
        float d = max(abs(local.x - starPos.x), abs(local.y - starPos.y));
        if (d < 0.6) {
            float twinkle = pow(0.5 + 0.5 * sin(iTime * (1.4 + rnd * 2.6) + rnd * 40.0), 3.0);
            vec3 starCol = (rnd > 0.92) ? vec3(1.0, 0.51, 0.72) :
                           (rnd > 0.83) ? vec3(0.40, 0.94, 1.0) :
                                          vec3(0.95, 0.97, 1.0);
            starAdd = starCol * twinkle * 0.45 * skyMask * bgMask;
        }
    }

    // 4. Periodic 8-bit diagonal pixel shooting star across the quiet left night sky
    vec3 meteorAdd = vec3(0.0);
    float cycle  = 6.0;
    float idx    = floor(iTime / cycle);
    float tLocal = fract(iTime / cycle);
    if (tLocal < 0.24) {
        float prog = tLocal / 0.24;
        vec2 startP = vec2(0.10 + hash21(vec2(idx, 3.1)) * 0.24,
                           0.08 + hash21(vec2(idx, 9.7)) * 0.16);
        vec2 dir  = normalize(vec2(0.86, 0.51));
        vec2 head = startP + dir * (prog * 0.34);

        vec2 qUV = (floor(fragCoord / pixSize) + 0.5) * pixSize / res;
        vec2 rel = qUV - head;
        float along = dot(rel, -dir);
        float perp  = abs(dot(rel, vec2(-dir.y, dir.x)));
        float tailLen = 0.085;
        if (along >= 0.0 && along < tailLen && perp < (pixSize * 0.75 / res.y)) {
            float fade = (1.0 - along / tailLen) * sin(prog * 3.14159265);
            vec3 meteorCol = mix(vec3(0.98, 0.99, 1.0), vec3(0.40, 0.94, 1.0), along / tailLen);
            meteorAdd = meteorCol * fade * 0.82 * (1.0 - smoothstep(0.55, 0.70, uv.x)) * bgMask;
        }
    }

    vec3 outRgb = clamp(col + oceanShimmer + glowAdd + starAdd + meteorAdd, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
