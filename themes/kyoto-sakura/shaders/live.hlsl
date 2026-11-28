// ============================================================================
// Kiseki Terminal — Kyoto Sakura Live Fantasy Shader (Windows Terminal)
//
// Drifting & tumbling sakura petals, meandering golden shrine fireflies,
// warm flickering stone lantern glow, and tranquil moonlit pond ripples.
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
    p = frac(p * float2(127.1, 311.7));
    p += dot(p, p + 34.5);
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

float kiseki_sakura_layer(float2 dipUV, float gridScale, float speed, float seed) {
    float2 p = dipUV * gridScale;
    p.x += Time * speed * 0.55;
    p.y -= Time * speed * 0.42;

    float2 id = floor(p);
    float2 f = frac(p) - 0.5;
    float h = kiseki_hash21(id + seed);
    if (h < 0.62) {
        return 0.0;
    }

    float phase = h * 31.4159 + Time * (1.2 + h * 1.4);
    float2 offset = float2(sin(phase * 0.7) * 0.24, cos(phase * 0.9) * 0.20);
    float2 q = f - offset;

    float ang = phase * 0.85;
    float ca = cos(ang);
    float sa = sin(ang);
    float2 rq = float2(q.x * ca - q.y * sa, q.x * sa + q.y * ca);
    float flutter = 0.45 + 0.55 * abs(sin(phase * 1.3));
    float d = length(float2(rq.x * 1.2, rq.y * (2.8 / max(flutter, 0.001))));
    return (1.0 - smoothstep(0.026, 0.072, d)) * (0.55 + 0.45 * h);
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
    float2 imgUV = kiseki_cover_uv(uv, texSize, imgSize, 0.36);

    float narrow = kiseki_narrow_factor(texSize);
    float bgOpacity = KISEKI_BG_OPACITY * lerp(0.28, 1.0, narrow);
    float texelPerPx = min(imgSize.x / max(texSize.x, 1.0), imgSize.y / max(texSize.y, 1.0));

    // --- 1. Tranquil moonlit temple pond horizontal shimmer -----------------
#if KISEKI_QUALITY_LEVEL >= 2
    float pondMask = smoothstep(0.46, 0.62, imgUV.x) * smoothstep(0.76, 0.86, imgUV.y) * narrow;
    float rippleX = sin(imgUV.y * 125.0 - Time * 1.8 + imgUV.x * 18.0)
        * (KISEKI_PARAM_POND_RIPPLE_AMP * 1.35) * pondMask;
    float rippleY = cos(imgUV.x * 70.0 + Time * 1.3)
        * (KISEKI_PARAM_POND_RIPPLE_AMP * 0.56) * pondMask;
    float3 wall = kiseki_sample_wall(imgUV + float2(rippleX, rippleY), imgSize, texelPerPx);
#else
    float3 wall = kiseki_sample_wall(imgUV, imgSize, texelPerPx);
#endif

    // --- 2. Warm organic stone lantern candle-glow flicker ------------------
    float lanternDist = length((imgUV - float2(0.615, 0.44)) * float2(1.6, 1.0));
    float lanternHalo = exp(-lanternDist * 6.5) * narrow;
    float flicker = 0.55
        + 0.28 * sin(Time * KISEKI_PARAM_LANTERN_FLICKER_HZ)
        + 0.17 * sin(Time * 6.1 + 1.4);
    wall += KISEKI_ACCENT_SECONDARY * lanternHalo * (0.16 * flicker);

    float3 animBg = lerp(KISEKI_BASE_BG, wall, bgOpacity);

    // --- 3. Parallax layers of drifting & fluttering sakura petals ----------
    float dipScale = 900.0 * max(Scale, 1.0);
    float2 dipUV = pos.xy / dipScale;
    float2 dipSize = texSize / dipScale;
    float speed = KISEKI_PARAM_PETAL_SPEED;
    float petalsFar = kiseki_sakura_layer(dipUV, 5.6, 0.28 * speed, 3.7);
    animBg += KISEKI_ACCENT_PRIMARY * (petalsFar * 0.26);

#if KISEKI_QUALITY_LEVEL >= 2
    float petalsNear = kiseki_sakura_layer(dipUV, 3.8, 0.42 * speed, 11.9);
    float3 sakuraLight = lerp(KISEKI_ACCENT_PRIMARY, float3(1.0, 0.94, 0.97), 0.55);
    animBg += sakuraLight * (petalsNear * 0.34);
#endif

    // --- 4. Meandering golden shrine fireflies (DIP-sized orbs) -------------
#if KISEKI_QUALITY_LEVEL >= 2
    [unroll]
    for (int i = 0; i < 6; i++) {
        float fi = (float)i;
        float2 center = float2(
            0.22 + 0.52 * frac(sin(fi * 12.9898) * 43758.5453),
            0.24 + 0.56 * frac(sin(fi * 78.2330) * 43758.5453)
        );
        float2 wander = float2(
            sin(Time * (0.28 + fi * 0.06) + fi * 2.1) * 0.075,
            cos(Time * (0.22 + fi * 0.05) + fi * 1.7) * 0.055
        );
        float2 fp = (center + wander) * dipSize;
        float d = length(dipUV - fp);
        float pulse = pow(0.5 + 0.5 * sin(Time * (1.1 + fi * 0.25) + fi * 3.7), 2.5);
        float orb = exp(-d * 130.0) * 0.55 + exp(-d * 34.0) * 0.14;
        animBg += KISEKI_ACCENT_SECONDARY * orb * pulse;
    }
#else
    [unroll]
    for (int i = 0; i < 3; i++) {
        float fi = (float)i;
        float2 center = float2(
            0.22 + 0.52 * frac(sin(fi * 12.9898) * 43758.5453),
            0.24 + 0.56 * frac(sin(fi * 78.2330) * 43758.5453)
        );
        float2 wander = float2(
            sin(Time * (0.28 + fi * 0.06) + fi * 2.1) * 0.075,
            cos(Time * (0.22 + fi * 0.05) + fi * 1.7) * 0.055
        );
        float2 fp = (center + wander) * dipSize;
        float d = length(dipUV - fp);
        float pulse = pow(0.5 + 0.5 * sin(Time * (1.1 + fi * 0.25) + fi * 3.7), 2.5);
        float orb = exp(-d * 130.0) * 0.55 + exp(-d * 34.0) * 0.14;
        animBg += KISEKI_ACCENT_SECONDARY * orb * pulse;
    }
#endif

    // --- 5. Exact Porter-Duff Source-Over compositing -----------------------
    float4 bgPremul = float4(saturate(animBg) * KISEKI_WINDOW_ALPHA, KISEKI_WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a = term.a + bgPremul.a * (1.0 - term.a);
    return saturate(outColor);
}
