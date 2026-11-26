// ============================================================================
// Kiseki Terminal — Shinkai Rain Live Cinematic Shader (Windows Terminal)
//
// Parallax diagonal Tokyo night rain streaks, wet puddle ripple refraction,
// and warm konbini/streetlamp bokeh breathing — code text stays razor sharp.
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

float kiseki_hash21(float2 p) {
    p = frac(p * float2(234.34, 435.345));
    p += dot(p, p + 34.23);
    return frac(p.x * p.y);
}

float2 kiseki_cover_uv(float2 uv, float2 screenSize, float2 imgSize) {
    float screenAspect = screenSize.x / max(screenSize.y, 1.0);
    float imgAspect = imgSize.x / max(imgSize.y, 1.0);
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

float kiseki_rain_layer(float2 uv, float cols, float speed, float seed) {
    float2 ruv = float2(uv.x + uv.y * 0.22, uv.y);
    ruv.x *= cols;
    float colId = floor(ruv.x);
    float xLocal = frac(ruv.x) - 0.5;

    float h = kiseki_hash21(float2(colId, seed));
    float dropSpeed = speed * (0.80 + 0.45 * h);
    float yScroll = ruv.y * 2.4 - Time * dropSpeed + h * 15.0;
    float rowId = floor(yScroll);
    float yLocal = frac(yScroll);

    float h2 = kiseki_hash21(float2(colId, rowId + seed * 31.7));
    if (h2 < 0.52) {
        return 0.0;
    }

    float xOff = (h2 - 0.76) * 0.55;
    float streakX = 1.0 - smoothstep(0.0, 0.055, abs(xLocal - xOff));
    float streakY = smoothstep(0.0, 0.65, yLocal) * (1.0 - smoothstep(0.84, 1.0, yLocal));
    return streakX * streakY * (0.45 + 0.55 * h);
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgUV = kiseki_cover_uv(tex, Resolution, float2(max(imgW, 1.0), max(imgH, 1.0)));

    float narrow = kiseki_narrow_factor(Resolution);
    float aspect = Resolution.x / max(Resolution.y, 1.0);

    // --- 1. Subtle wet pavement & puddle ripple refraction ------------------
    float puddleMask = smoothstep(0.79, 0.86, imgUV.x) * smoothstep(0.72, 0.82, imgUV.y) * narrow;
#if KISEKI_QUALITY_LEVEL >= 2
    float rip1 = sin(imgUV.y * 140.0 - Time * 3.2 + imgUV.x * 35.0);
    float rip2 = cos(imgUV.x * 95.0 + Time * 2.4 - imgUV.y * 60.0);
    float2 puddleOff = float2(rip1 * 0.0016, rip2 * 0.0010) * puddleMask;
    float3 wall = imageTexture.Sample(samplerState, saturate(imgUV + puddleOff)).rgb;
#else
    float3 wall = imageTexture.Sample(samplerState, imgUV).rgb;
#endif

    // --- 2. Soft warm streetlamp & turquoise konbini bokeh breathing glow ---
    float luma = dot(wall, float3(0.299, 0.587, 0.114));
    float bokehMask = smoothstep(0.48, 0.88, luma) * smoothstep(0.48, 0.66, imgUV.x) * narrow;
    float breathe = 0.5 + 0.5 * sin(Time * KISEKI_PARAM_BOKEH_PULSE_HZ + imgUV.x * 3.0);
    wall += wall * bokehMask * (0.14 * breathe);

    float3 animBg = lerp(KISEKI_BASE_BG, wall, KISEKI_BG_OPACITY);

    // --- 3. Parallax falling Tokyo night rain streaks -----------------------
    float2 aspectUV = float2(tex.x * aspect, tex.y);
    float speed = KISEKI_PARAM_RAIN_SPEED;
    float rainFar = kiseki_rain_layer(aspectUV, 42.0, 1.35 * speed, 1.0);

    float3 aquaRain = lerp(KISEKI_ACCENT_PRIMARY, float3(0.55, 0.88, 0.96), 0.35);
    float rightWarm = smoothstep(0.55, 0.90, tex.x) * 0.35;
    float3 rainCol = lerp(aquaRain, KISEKI_ACCENT_SECONDARY, rightWarm);
    animBg += rainCol * (rainFar * 0.14);

#if KISEKI_QUALITY_LEVEL >= 2
    float rainNear = kiseki_rain_layer(aspectUV, 26.0, 2.05 * speed, 7.3);
    animBg += rainCol * (rainNear * 0.20);

    // --- 4. Concentric puddle raindrop ring highlights in wet walkway -------
    if (puddleMask > 0.05) {
        float2 pCell = floor(aspectUV * 10.0);
        float2 pLoc = frac(aspectUV * 10.0) - 0.5;
        float ph = kiseki_hash21(pCell + 19.1);
        float pt = frac(Time * KISEKI_PARAM_PUDDLE_RING_RATE + ph);
        float rad = pt * 0.42;
        float dist = length(float2(pLoc.x, pLoc.y * 2.2));
        float ring = exp(-abs(dist - rad) * 45.0) * (1.0 - pt) * smoothstep(0.0, 0.1, pt);
        animBg += KISEKI_ACCENT_PRIMARY * ring * puddleMask * 0.22;
    }
#endif

    // --- 5. Optional CRT scanlines ------------------------------------------
#if KISEKI_ENABLE_CRT
    float microScan = 0.94 + 0.06 * (0.5 + 0.5 * sin(pos.y * 1.5707963));
    animBg *= microScan;
#endif

    // --- 6. Exact Porter-Duff Source-Over compositing -----------------------
    float4 bgPremul = float4(saturate(animBg) * KISEKI_WINDOW_ALPHA, KISEKI_WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a = term.a + bgPremul.a * (1.0 - term.a);
    return saturate(outColor);
}
