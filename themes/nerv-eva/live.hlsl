// ============================================================================
// ★ Kiseki Terminal — NERV UNIT-02 Live Tactical Shader (60fps GPU)
// Homage to nerv-base: sweeping MAGI amber scanline, AT-Field hex radar pulse,
// crimson blueprint heartbeat, and periodic horizontal-slice RGB glitch bursts.
// ============================================================================

Texture2D shaderTexture : register(t0);
Texture2D imageTexture  : register(t1);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings : register(b0) {
    float  Time;
    float  Scale;
    float2 Resolution;
    float4 Background;
};

static const float3 BASE_BG      = float3(0.0431, 0.0353, 0.0392); // #0B090A
static const float  BG_OPACITY   = 0.86;
static const float  WINDOW_ALPHA = 0.95;

float hash11(float p) {
    p = frac(p * 0.1031);
    p *= p + 33.33;
    p *= p + p;
    return frac(p);
}

float2 GetBottomRightCoverUV(float2 uv, float2 screenSize, float2 imgSize) {
    float screenAspect = screenSize.x / max(screenSize.y, 1.0);
    float imgAspect    = imgSize.x / max(imgSize.y, 1.0);
    float2 outUV = uv;
    if (screenAspect > imgAspect) {
        float visH = imgAspect / screenAspect;
        outUV.y = (1.0 - visH) + uv.y * visH;
    } else {
        float visW = screenAspect / imgAspect;
        outUV.x = (1.0 - visW) + uv.x * visW;
    }
    return saturate(outUV);
}

float HexEdge(float2 p) {
    p.x *= 1.1547005;
    float isTwo = frac(floor(p.x) * 0.5) * 2.0;
    p.y += isTwo * 0.5;
    float2 f = abs(frac(p) - 0.5);
    float d = max(f.x * 1.5 + f.y * 0.866025, f.y * 1.73205);
    return smoothstep(0.81, 0.86, d);
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgUV = GetBottomRightCoverUV(tex, Resolution, float2(max(imgW, 1.0), max(imgH, 1.0)));

    // 1. Periodic micro RGB horizontal-slice glitch burst (every 6.0s, ~0.17s) — background only
    float glitchCycle = frac(Time / 6.0);
    float glitchOn = step(glitchCycle, 0.028);
    float stepTick = floor(Time * 28.0);
    float bandA = step(0.28, tex.y) * step(tex.y, 0.34);
    float bandB = step(0.62, tex.y) * step(tex.y, 0.67);
    float sliceMask = saturate(bandA + bandB) * glitchOn;
    float sliceShift = (hash11(stepTick + bandA * 7.0) - 0.5) * 0.014 * sliceMask;
    float chromaOff  = (0.0028 * glitchOn) + (0.0045 * sliceMask);

    float2 uvBase = saturate(imgUV + float2(sliceShift, 0.0));
    float3 wall;
    wall.r = imageTexture.Sample(samplerState, saturate(uvBase + float2(-chromaOff, 0.0))).r;
    wall.g = imageTexture.Sample(samplerState, uvBase).g;
    wall.b = imageTexture.Sample(samplerState, saturate(uvBase + float2( chromaOff, 0.0))).b;

    // 2. Unit-02 blueprint wireframe sync-rate breathing pulse
    float lineEnergy = saturate(max(wall.r - BASE_BG.r, max(wall.g - BASE_BG.g, wall.b - BASE_BG.b)) * 2.5);
    float heartbeat  = 0.5 + 0.5 * sin(Time * 1.9);
    wall += wall * lineEnergy * (0.20 * heartbeat);

    // 3. Slow sweeping MAGI amber tactical scan beam (7.5s period)
    float scanY = frac(Time / 7.5);
    float dScan = abs(tex.y - scanY);
    float scanCore = exp(-dScan * 220.0);
    float scanTail = smoothstep(0.08, 0.0, scanY - tex.y) * step(tex.y, scanY) * 0.25;

    float3 magiAmber = float3(0.94, 0.42, 0.13); // #F06A22
    float3 nervRed   = float3(0.85, 0.15, 0.18); // #D8262E
    float3 animBg = lerp(BASE_BG, wall, BG_OPACITY);
    animBg += magiAmber * (scanCore * 0.16 + scanTail * 0.05);
    animBg += magiAmber * lineEnergy * (scanCore * 0.70 + scanTail * 0.45);

    // 4. Subtle AT-Field hexagonal radar shimmer anchored on right half
    float2 hexCoord = (pos.xy / (46.0 * max(Scale, 1.0)));
    float hexLine = HexEdge(hexCoord);
    float hexZone = smoothstep(0.45, 0.82, tex.x) * (1.0 - smoothstep(0.0, 0.12, abs(tex.y - 0.5) - 0.38));
    float hexWave = 0.5 + 0.5 * sin(Time * 1.1 - tex.x * 4.0 + tex.y * 2.5);
    animBg += lerp(nervRed, magiAmber, hexWave) * hexLine * hexZone * (0.05 * hexWave);

    // 5. Whisper-soft 3% tactical CRT micro-scanlines on background
    float microScan = 0.97 + 0.03 * (0.5 + 0.5 * sin(pos.y * 3.14159265));
    animBg *= microScan;

    // 6. Exact Porter-Duff Source-Over compositing (AtlasEngine t0 uses premultiplied alpha with empty=0)
    float4 bgPremul = float4(saturate(animBg) * WINDOW_ALPHA, WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a   = term.a   + bgPremul.a   * (1.0 - term.a);
    return saturate(outColor);
}
