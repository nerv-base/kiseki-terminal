// ============================================================================
// Kiseki Terminal — NERV UNIT-02 Live Tactical Shader (Windows Terminal layered bg)
//
// Homage to nerv-base: sweeping MAGI amber scanline, AT-Field hex radar pulse,
// crimson blueprint heartbeat, and periodic horizontal-slice RGB glitch bursts.
//
// This is a *template*. kiseki-shader injects the KISEKI_* macro header before
// validation and compilation, so no color, opacity or geometry constant is
// hard-coded here.
//
// Windows Terminal hands us two independent layers:
//   t0 `shaderTexture` — text, empty pixels have alpha 0
//   t1 `imageTexture`  — wallpaper, sampled independently
// Effects therefore only ever touch the wallpaper layer, then the result is
// composited Source-Over so text pixels are never modified.
//
// Constraints enforced by `kiseki theme validate`:
//   * every smoothstep(a, b, x) has a < b (no reversed edges)
//   * loop bounds are compile-time constants
//   * texture samples stay within the per-profile budget
// ============================================================================

Texture2D shaderTexture : register(t0);
Texture2D imageTexture : register(t1);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings : register(b0) {
    float Time;
    float Scale;
    float2 Resolution;
    float4 Background;
};

float kiseki_hash11(float p) {
    p = frac(p * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return frac(p);
}

// Right-anchored "cover" fit with configurable vertical focal anchor.
float2 kiseki_cover_uv(float2 uv, float2 screenSize, float2 imgSize, float anchorY) {
    float screenAspect = screenSize.x / max(screenSize.y, 1.0);
    float imgAspect = imgSize.x / max(imgSize.y, 1.0);
    float2 outUV = uv;
    if (screenAspect > imgAspect) {
        float visH = imgAspect / max(screenAspect, 0.0001);
        outUV.y = (1.0 - visH) * anchorY + uv.y * visH;
    } else {
        float visW = screenAspect / max(imgAspect, 0.0001);
        outUV.x = (1.0 - visW) + uv.x * visW;
    }
    return saturate(outUV);
}

float kiseki_hex_edge(float2 p) {
    p.x *= 1.1547005;
    float isTwo = frac(floor(p.x) * 0.5) * 2.0;
    p.y += isTwo * 0.5;
    float2 f = abs(frac(p) - 0.5);
    float d = max(f.x * 1.5 + f.y * 0.866025, f.y * 1.73205);
    return smoothstep(0.81, 0.86, d);
}

// Aspect-ratio fade is injected by Kiseki as `kiseki_narrow_factor`; the
// template must not redefine it.

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float texW, texH;
    shaderTexture.GetDimensions(texW, texH);
    float2 texSize = max(float2(texW, texH), float2(1.0, 1.0));
    float2 uv = saturate(pos.xy / texSize);
    float4 term = shaderTexture.Load(int3(pos.xy, 0));

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgSize = max(float2(imgW, imgH), float2(1.0, 1.0));
    float2 imgUV = kiseki_cover_uv(uv, texSize, imgSize, 0.45);

    float narrow = kiseki_narrow_factor(texSize);
    float bgOpacity = KISEKI_BG_OPACITY * lerp(0.28, 1.0, narrow);

    // --- 1. Periodic micro RGB horizontal-slice glitch burst ---------------
#if KISEKI_QUALITY_LEVEL >= 2
    float glitchCycle = frac(Time / KISEKI_PARAM_GLITCH_INTERVAL_SEC);
    float glitchOn = step(glitchCycle, 0.028);
    float stepTick = floor(Time * 28.0);
    float bandA = step(0.28, uv.y) * step(uv.y, 0.34);
    float bandB = step(0.62, uv.y) * step(uv.y, 0.67);
    float sliceMask = saturate(bandA + bandB) * glitchOn * narrow;
    float sliceShift = (kiseki_hash11(stepTick + bandA * 7.0) - 0.5) * 0.014 * sliceMask;
    float chromaOff = (0.0028 * glitchOn) + (0.0045 * sliceMask);

    float2 uvBase = saturate(imgUV + float2(sliceShift, 0.0));
    float3 wall;
    wall.r = imageTexture.Sample(samplerState, saturate(uvBase + float2(-chromaOff, 0.0))).r;
    wall.g = imageTexture.Sample(samplerState, uvBase).g;
    wall.b = imageTexture.Sample(samplerState, saturate(uvBase + float2(chromaOff, 0.0))).b;
#else
    float3 wall = imageTexture.Sample(samplerState, imgUV).rgb;
#endif

    // --- 2. Unit-02 blueprint wireframe breathing pulse -------------------
    float lineEnergy = saturate(
        max(wall.r - KISEKI_BASE_BG.r, max(wall.g - KISEKI_BASE_BG.g, wall.b - KISEKI_BASE_BG.b)) * 2.5
    ) * narrow;
    float heartbeat = 0.5 + 0.5 * sin(Time * KISEKI_PARAM_HEARTBEAT_HZ);
    wall += wall * lineEnergy * (0.20 * heartbeat);

    // --- 3. Slow sweeping MAGI amber tactical scan beam (DIP thickness) ---
    float dipScale = 900.0 * max(Scale, 1.0);
    float dipH = texSize.y / dipScale;
    float scanY = frac(Time / KISEKI_PARAM_SCAN_PERIOD_SEC);
    float dScanDip = abs(uv.y - scanY) * dipH;
    float scanCore = exp(-dScanDip * 220.0);
    // Edges ordered low->high; the tail is a masked band, never a reversed ramp.
    float scanTail = (1.0 - smoothstep(0.0, 0.08, (scanY - uv.y) * dipH)) * step(uv.y, scanY) * 0.25;

    float3 animBg = lerp(KISEKI_BASE_BG, wall, bgOpacity);
    animBg += KISEKI_ACCENT_PRIMARY * (scanCore * 0.16 + scanTail * 0.05) * narrow;
    animBg += KISEKI_ACCENT_PRIMARY * lineEnergy * (scanCore * 0.70 + scanTail * 0.45);

    // --- 4. AT-Field hexagonal radar shimmer on the art half --------------
    float2 hexCoord = pos.xy / (KISEKI_PARAM_HEX_CELL_PX * max(Scale, 1.0));
    float hexLine = kiseki_hex_edge(hexCoord);
    float hexZone = smoothstep(0.45, 0.82, uv.x)
        * (1.0 - smoothstep(0.0, 0.12, abs(uv.y - 0.5) - 0.38))
        * narrow;
    float hexWave = 0.5 + 0.5 * sin(Time * 1.1 - uv.x * 4.0 + uv.y * 2.5);
    animBg += lerp(KISEKI_ACCENT_SECONDARY, KISEKI_ACCENT_PRIMARY, hexWave)
        * hexLine * hexZone * (0.05 * hexWave);

    // --- 5. Exact Porter-Duff Source-Over (t0 premultiplied, empty = 0) ---
    float4 bgPremul = float4(saturate(animBg) * KISEKI_WINDOW_ALPHA, KISEKI_WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a = term.a + bgPremul.a * (1.0 - term.a);
    return saturate(outColor);
}
