// ============================================================================
// ★ Kiseki Terminal — NERV UNIT-02 Live Tactical Shader for Ghostty (60fps GPU)
// Homage to nerv-base: sweeping MAGI amber scanline, AT-Field hex radar pulse,
// crimson blueprint heartbeat, and periodic tactical glitch accents.
// ============================================================================

const vec3 BASE_BG = vec3(0.0431, 0.0353, 0.0392); // #0B090A

float hash11(float p) {
    p = fract(p * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return fract(p);
}

float HexEdge(vec2 p) {
    p.x *= 1.1547005;
    float isTwo = fract(floor(p.x) * 0.5) * 2.0;
    p.y += isTwo * 0.5;
    vec2 f = abs(fract(p) - 0.5);
    float d = max(f.x * 1.5 + f.y * 0.866025, f.y * 1.73205);
    return smoothstep(0.81, 0.86, d);
}

float bgLayerMask(vec4 term, vec2 uv) {
    vec3 unpremul = term.rgb / max(term.a, 0.001);
    float diff = length(unpremul - BASE_BG);
    float luma = dot(unpremul, vec3(0.299, 0.587, 0.114));
    float codeZone = 1.0 - smoothstep(0.54, 0.74, uv.x);
    float glyph = smoothstep(0.22, 0.48, luma) * smoothstep(0.18, 0.42, diff);
    return 1.0 - glyph * mix(0.42, 0.88, codeZone);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 res = max(iResolution.xy, vec2(1.0));
    vec2 uv  = fragCoord / res;
    vec4 term = texture(iChannel0, uv);
    vec3 col  = term.rgb / max(term.a, 0.001);

    float bgMask = bgLayerMask(term, uv);
    float scale  = max(1.0, round(min(res.x, res.y) / 720.0));

    // 1. Periodic micro RGB tactical glitch accent on right artwork zone only (every 6.0s, ~0.17s)
    float glitchCycle = fract(iTime / 6.0);
    float glitchOn    = step(glitchCycle, 0.028);
    float stepTick    = floor(iTime * 28.0);
    float bandA       = step(0.28, uv.y) * step(uv.y, 0.34);
    float bandB       = step(0.62, uv.y) * step(uv.y, 0.67);
    float rightOnly   = smoothstep(0.56, 0.68, uv.x) * bgMask;
    float sliceMask   = clamp(bandA + bandB, 0.0, 1.0) * glitchOn * rightOnly;
    if (sliceMask > 0.01) {
        float sliceShift = (hash11(stepTick + bandA * 7.0) - 0.5) * 0.010 * sliceMask;
        float chromaOff  = 0.0032 * sliceMask;
        vec2 uvBase = clamp(uv + vec2(sliceShift, 0.0), 0.0, 1.0);
        vec4 sR = texture(iChannel0, clamp(uvBase + vec2(-chromaOff, 0.0), 0.0, 1.0));
        vec4 sG = texture(iChannel0, uvBase);
        vec4 sB = texture(iChannel0, clamp(uvBase + vec2( chromaOff, 0.0), 0.0, 1.0));
        vec3 glitched = vec3(sR.r / max(sR.a, 0.001), sG.g / max(sG.a, 0.001), sB.b / max(sB.a, 0.001));
        col = mix(col, glitched, sliceMask);
    }

    // 2. Unit-02 blueprint wireframe sync-rate breathing pulse (right artwork area)
    float artZone    = smoothstep(0.50, 0.68, uv.x) * bgMask;
    float lineEnergy = clamp(max(col.r - BASE_BG.r, max(col.g - BASE_BG.g, col.b - BASE_BG.b)) * 2.2, 0.0, 1.0) * artZone;
    float heartbeat  = 0.5 + 0.5 * sin(iTime * 1.9);
    col += col * lineEnergy * (0.18 * heartbeat);

    // 3. Slow sweeping MAGI amber tactical scan beam (7.5s period)
    float scanY    = fract(iTime / 7.5);
    float dScan    = abs(uv.y - scanY);
    float scanCore = exp(-dScan * 220.0);
    float scanTail = smoothstep(0.08, 0.0, scanY - uv.y) * step(uv.y, scanY) * 0.25;

    vec3 magiAmber = vec3(0.94, 0.42, 0.13); // #F06A22
    vec3 nervRed   = vec3(0.85, 0.15, 0.18); // #D8262E
    col += magiAmber * (scanCore * 0.15 + scanTail * 0.045) * bgMask;
    col += magiAmber * lineEnergy * (scanCore * 0.60 + scanTail * 0.35);

    // 4. Subtle AT-Field hexagonal radar shimmer anchored on right half
    vec2 hexCoord = fragCoord / (46.0 * scale);
    float hexLine = HexEdge(hexCoord);
    float hexZone = smoothstep(0.45, 0.82, uv.x) * (1.0 - smoothstep(0.0, 0.12, abs(uv.y - 0.5) - 0.38));
    float hexWave = 0.5 + 0.5 * sin(iTime * 1.1 - uv.x * 4.0 + uv.y * 2.5);
    col += mix(nervRed, magiAmber, hexWave) * hexLine * hexZone * (0.055 * hexWave) * bgMask;

    // 5. Whisper-soft 3% tactical CRT micro-scanlines on background
    float microScan = mix(1.0, 0.97 + 0.03 * (0.5 + 0.5 * sin(fragCoord.y * 3.14159265)), bgMask);
    col *= microScan;

    vec3 outRgb = clamp(col, 0.0, 1.0);
    fragColor = vec4(outRgb * term.a, term.a);
}
