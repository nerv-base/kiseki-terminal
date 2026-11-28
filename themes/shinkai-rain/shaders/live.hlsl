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

float3 kiseki_sample_wall(float2 sampleUV, float2 imgSize, float texelPerPx) {
    float3 result;
#if KISEKI_QUALITY_LEVEL >= 2
    if (texelPerPx > 1.35) {
        float2 stepUV = texelPerPx / max(imgSize, float2(1.0, 1.0));
        float3 c0 = imageTexture.Sample(samplerState, saturate(sampleUV + float2(-0.125, -0.375) * stepUV)).rgb;
        float3 c1 = imageTexture.Sample(samplerState, saturate(sampleUV + float2(0.375, -0.125) * stepUV)).rgb;
        float3 c2 = imageTexture.Sample(samplerState, saturate(sampleUV + float2(-0.375, 0.125) * stepUV)).rgb;
        float3 c3 = imageTexture.Sample(samplerState, saturate(sampleUV + float2(0.125, 0.375) * stepUV)).rgb;
        result = 0.25 * (c0 + c1 + c2 + c3);
    } else {
        result = imageTexture.Sample(samplerState, saturate(sampleUV)).rgb;
    }
#else
    result = imageTexture.Sample(samplerState, saturate(sampleUV)).rgb;
#endif
    return result;
}

float kiseki_rain_layer(float2 dipUV, float cols, float speed, float seed, float halfWidth) {
    float2 ruv = float2(dipUV.x + dipUV.y * 0.22, dipUV.y);
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
    float streakX = 1.0 - smoothstep(0.0, halfWidth, abs(xLocal - xOff));
    float streakY = smoothstep(0.0, 0.65, yLocal) * (1.0 - smoothstep(0.84, 1.0, yLocal));
    return streakX * streakY * (0.45 + 0.55 * h);
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float texW, texH;
    shaderTexture.GetDimensions(texW, texH);
    float2 texSize = max(float2(texW, texH), float2(1.0, 1.0));
    float2 uv = saturate(pos.xy / texSize);
    float4 term = shaderTexture.Load(int3(pos.xy, 0));

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgSize = max(float2(imgW, imgH), float2(1.0, 1.0));
    float2 imgUV = kiseki_cover_uv(uv, texSize, imgSize, 0.32);

    float narrow = kiseki_narrow_factor(texSize);
    float bgOpacity = KISEKI_BG_OPACITY * lerp(0.28, 1.0, narrow);
    float texelPerPx = min(imgSize.x / max(texSize.x, 1.0), imgSize.y / max(texSize.y, 1.0));

    // --- 1. Subtle wet pavement & puddle ripple refraction ------------------
    float puddleMask = smoothstep(0.79, 0.86, imgUV.x) * smoothstep(0.72, 0.82, imgUV.y) * narrow;
#if KISEKI_QUALITY_LEVEL >= 2
    float rip1 = sin(imgUV.y * 140.0 - Time * 3.2 + imgUV.x * 35.0);
    float rip2 = cos(imgUV.x * 95.0 + Time * 2.4 - imgUV.y * 60.0);
    float2 puddleOff = float2(rip1 * 0.0016, rip2 * 0.0010) * puddleMask;
    float3 wall = kiseki_sample_wall(imgUV + puddleOff, imgSize, texelPerPx);
#else
    float3 wall = kiseki_sample_wall(imgUV, imgSize, texelPerPx);
#endif

    // --- 2. Soft warm streetlamp & turquoise konbini bokeh breathing glow ---
    float luma = dot(wall, float3(0.299, 0.587, 0.114));
    float bokehMask = smoothstep(0.48, 0.88, luma) * smoothstep(0.48, 0.66, imgUV.x) * narrow;
    float breathe = 0.5 + 0.5 * sin(Time * KISEKI_PARAM_BOKEH_PULSE_HZ + imgUV.x * 3.0);
    wall += wall * bokehMask * (0.14 * breathe);

    float3 animBg = lerp(KISEKI_BASE_BG, wall, bgOpacity);

    // --- 3. Parallax falling Tokyo night rain streaks (DIP-normalized) ------
    float dpiScale = max(Scale, 1.0);
    float dipScale = 900.0 * dpiScale;
    float2 dipUV = pos.xy / dipScale;
    float speed = KISEKI_PARAM_RAIN_SPEED;
    float codeRainFade = lerp(
        0.52,
        1.0,
        smoothstep(KISEKI_SAFE_ZONE_END_X - 0.14, KISEKI_SAFE_ZONE_END_X + 0.16, uv.x)
    );

    float farHalf = max(min(0.042, min(0.95 * dpiScale, 1.45) * 46.0 / dipScale), 0.004);
    float rainFar = kiseki_rain_layer(dipUV, 46.0, 1.35 * speed, 1.0, farHalf);

    float3 aquaRain = lerp(KISEKI_ACCENT_PRIMARY, float3(0.55, 0.88, 0.96), 0.35);
    float rightWarm = smoothstep(0.55, 0.90, uv.x) * 0.35;
    float3 rainCol = lerp(aquaRain, KISEKI_ACCENT_SECONDARY, rightWarm);
    animBg += rainCol * (rainFar * 0.14) * codeRainFade;

#if KISEKI_QUALITY_LEVEL >= 2
    float nearHalf = max(min(0.045, min(1.25 * dpiScale, 1.85) * 28.0 / dipScale), 0.004);
    float rainNear = kiseki_rain_layer(dipUV, 28.0, 2.05 * speed, 7.3, nearHalf);
    animBg += rainCol * (rainNear * 0.20) * codeRainFade;

    // --- 4. Concentric puddle raindrop ring highlights in wet walkway -------
    if (puddleMask > 0.05) {
        float2 pCell = floor(dipUV * 10.0);
        float2 pLoc = frac(dipUV * 10.0) - 0.5;
        float ph = kiseki_hash21(pCell + 19.1);
        float pt = frac(Time * KISEKI_PARAM_PUDDLE_RING_RATE + ph);
        float rad = pt * 0.42;
        float dist = length(float2(pLoc.x, pLoc.y * 2.2));
        float ring = exp(-abs(dist - rad) * 45.0) * (1.0 - pt) * smoothstep(0.0, 0.1, pt);
        animBg += KISEKI_ACCENT_PRIMARY * ring * puddleMask * 0.22;
    }
#endif

    // --- 5. Exact Porter-Duff Source-Over compositing -----------------------
    float4 bgPremul = float4(saturate(animBg) * KISEKI_WINDOW_ALPHA, KISEKI_WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a = term.a + bgPremul.a * (1.0 - term.a);
    return saturate(outColor);
}
