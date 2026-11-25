// ============================================================================
// ★ Kiseki Terminal — Shinkai Rain Live Cinematic Shader (60fps GPU)
// Parallax diagonal Tokyo night rain streaks, wet puddle ripple refraction,
// and warm konbini/streetlamp bokeh breathing — code text stays 100% razor sharp.
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

static const float BG_OPACITY = 0.44;

float hash21(float2 p) {
    p = frac(p * float2(234.34, 435.345));
    p += dot(p, p + 34.23);
    return frac(p.x * p.y);
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

// Single parallax layer of slanted translucent rain streaks
float RainLayer(float2 uv, float cols, float speed, float seed) {
    // Gentle 13-degree wind slant
    float2 ruv = float2(uv.x + uv.y * 0.22, uv.y);
    ruv.x *= cols;
    float colId = floor(ruv.x);
    float xLocal = frac(ruv.x) - 0.5;

    float h = hash21(float2(colId, seed));
    float dropSpeed = speed * (0.80 + 0.45 * h);
    float yScroll = ruv.y * 2.4 - Time * dropSpeed + h * 15.0;
    float rowId = floor(yScroll);
    float yLocal = frac(yScroll);

    float h2 = hash21(float2(colId, rowId + seed * 31.7));
    if (h2 < 0.52) return 0.0;

    float xOff = (h2 - 0.76) * 0.55;
    float streakX = smoothstep(0.055, 0.0, abs(xLocal - xOff));
    float streakY = smoothstep(0.0, 0.65, yLocal) * smoothstep(1.0, 0.84, yLocal);
    return streakX * streakY * (0.45 + 0.55 * h);
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);

    float bgAlpha = max(Background.a, 0.001);
    float3 baseBg = Background.rgb / bgAlpha;

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgUV = GetBottomRightCoverUV(tex, Resolution, float2(max(imgW, 1.0), max(imgH, 1.0)));

    // 1. Subtle wet pavement & puddle ripple refraction restricted strictly to wet walkway right of railing
    float puddleMask = smoothstep(0.79, 0.86, imgUV.x) * smoothstep(0.72, 0.82, imgUV.y);
    float rip1 = sin(imgUV.y * 140.0 - Time * 3.2 + imgUV.x * 35.0);
    float rip2 = cos(imgUV.x * 95.0  + Time * 2.4 - imgUV.y * 60.0);
    float2 puddleOff = float2(rip1 * 0.0016, rip2 * 0.0010) * puddleMask;
    float3 wall = imageTexture.Sample(samplerState, saturate(imgUV + puddleOff)).rgb;

    // 2. Soft warm streetlamp & turquoise konbini bokeh breathing glow
    float luma = dot(wall, float3(0.299, 0.587, 0.114));
    float bokehMask = smoothstep(0.48, 0.88, luma) * smoothstep(0.48, 0.66, imgUV.x);
    float breathe = 0.5 + 0.5 * sin(Time * 1.35 + imgUV.x * 3.0);
    wall += wall * bokehMask * (0.14 * breathe);

    float3 animBg = lerp(baseBg, wall, BG_OPACITY);

    // 3. Dual-layer parallax falling Tokyo night rain streaks
    float2 aspectUV = float2(tex.x * (Resolution.x / max(Resolution.y, 1.0)), tex.y);
    float rainFar  = RainLayer(aspectUV, 42.0, 1.35, 1.0);
    float rainNear = RainLayer(aspectUV, 26.0, 2.05, 7.3);

    float3 aquaRain = float3(0.45, 0.86, 0.92); // #73DCEB rain-washed aqua
    float3 warmTint = float3(1.00, 0.82, 0.52); // #FFD185 streetlamp reflection on right
    float rightWarm = smoothstep(0.55, 0.90, tex.x) * 0.35;
    float3 rainCol  = lerp(aquaRain, warmTint, rightWarm);

    animBg += rainCol * (rainFar * 0.11 + rainNear * 0.18);

    // 4. Tiny concentric puddle raindrop ring highlights in lower-right puddle
    if (puddleMask > 0.05) {
        float2 pCell = floor(aspectUV * 10.0);
        float2 pLoc  = frac(aspectUV * 10.0) - 0.5;
        float ph = hash21(pCell + 19.1);
        float pt = frac(Time * 0.55 + ph);
        float rad = pt * 0.42;
        float dist = length(float2(pLoc.x, pLoc.y * 2.2)); // perspective-squashed ellipse
        float ring = exp(-abs(dist - rad) * 45.0) * (1.0 - pt) * smoothstep(0.0, 0.1, pt);
        animBg += float3(0.36, 0.90, 0.82) * ring * puddleMask * 0.20;
    }

    // 5. Composite razor-sharp terminal text & HUD over animated background
    float3 diff = abs(term.rgb - Background.rgb);
    float maxDiff = max(diff.r, max(diff.g, diff.b));
    float alphaDiff = max(0.0, term.a - Background.a);
    float fgMask = saturate(max(smoothstep(0.012, 0.055, maxDiff), smoothstep(0.005, 0.035, alphaDiff)));

    float3 outRgb = term.rgb + (animBg - baseBg) * bgAlpha * (1.0 - fgMask);
    return float4(saturate(outRgb), max(term.a, bgAlpha));
}
