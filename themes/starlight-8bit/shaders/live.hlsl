// ============================================================================
// Kiseki Terminal — Starlight 8-Bit Live Pixel Shader (Windows Terminal)
//
// Real-time pixel-quantized twinkling stars, periodic diagonal shooting stars,
// shimmering pixel ocean waves, and cozy CRT/moonlight pulse — zero text blur.
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
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
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

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgUV = kiseki_cover_uv(tex, Resolution, float2(max(imgW, 1.0), max(imgH, 1.0)));

    float narrow = kiseki_narrow_factor(Resolution);

    // --- 1. Subtle pixel-stepped ocean wave shimmer -------------------------
#if KISEKI_QUALITY_LEVEL >= 2
    float oceanMask = smoothstep(0.54, 0.61, imgUV.y)
        * (1.0 - smoothstep(0.76, 0.85, imgUV.y))
        * smoothstep(0.32, 0.48, imgUV.x)
        * (1.0 - smoothstep(0.76, 0.86, imgUV.x))
        * narrow;
    float pixRow = floor(pos.y / 3.0);
    float waveShift = sin(pixRow * 0.65 + Time * KISEKI_PARAM_WAVE_SPEED) * 0.0022 * oceanMask;
    float2 warpedUV = saturate(imgUV + float2(waveShift, 0.0));
    float3 wall = imageTexture.Sample(samplerState, warpedUV).rgb;
#else
    float3 wall = imageTexture.Sample(samplerState, imgUV).rgb;
#endif

    // --- 2. Cozy breathing pulse on glowing cyan CRT monitor & moon ---------
    float rightZone = smoothstep(0.58, 0.78, imgUV.x) * narrow;
    float cyanGlow = saturate(wall.g * 0.6 + wall.b * 0.6 - wall.r * 0.5);
    float crtPulse = 0.5 + 0.5 * sin(Time * KISEKI_PARAM_CRT_PULSE_HZ);
    wall += wall * cyanGlow * rightZone * (0.14 * crtPulse);

    float3 animBg = lerp(KISEKI_BASE_BG, wall, KISEKI_BG_OPACITY);

    // --- 3. Crisp 3x3 pixel-grid twinkling stars in upper midnight sky ------
    float pixSize = max(2.0, round(3.0 * max(Scale, 1.0)));
    float2 pixCoord = floor(pos.xy / max(pixSize, 1.0));
    float2 cell = floor(pixCoord / 18.0);
    float2 local = pixCoord - cell * 18.0;
    float rnd = kiseki_hash21(cell);

    float skyMask = (1.0 - smoothstep(0.42, 0.58, tex.y)) * (1.0 - smoothstep(0.58, 0.78, tex.x));
    if (rnd > 0.74 && skyMask > 0.01) {
        float2 starPos = floor(float2(kiseki_hash21(cell + 17.3), kiseki_hash21(cell + 53.7)) * 14.0) + 2.0;
        float d = max(abs(local.x - starPos.x), abs(local.y - starPos.y));
        if (d < 0.6) {
            float twinkle = pow(0.5 + 0.5 * sin(Time * (1.4 + rnd * 2.6) + rnd * 40.0), 3.0);
            float3 starCol = (rnd > 0.92) ? KISEKI_ACCENT_SECONDARY :
                             (rnd > 0.83) ? KISEKI_ACCENT_PRIMARY :
                                            float3(0.95, 0.97, 1.0);
            animBg += starCol * twinkle * 0.45 * skyMask;
        }
    }

    // --- 4. Periodic 8-bit diagonal pixel shooting star ---------------------
#if KISEKI_QUALITY_LEVEL >= 2
    float cycle = max(KISEKI_PARAM_METEOR_CYCLE_SEC, 1.0);
    float idx = floor(Time / cycle);
    float tLocal = frac(Time / cycle);
    if (tLocal < 0.24) {
        float prog = tLocal / 0.24;
        float2 startP = float2(
            0.10 + kiseki_hash21(float2(idx, 3.1)) * 0.24,
            0.08 + kiseki_hash21(float2(idx, 9.7)) * 0.16
        );
        float2 dir = normalize(float2(0.86, 0.51));
        float2 head = startP + dir * (prog * 0.34);

        float2 qUV = (floor(pos.xy / max(pixSize, 1.0)) + 0.5) * pixSize / max(Resolution, float2(1.0, 1.0));
        float2 rel = qUV - head;
        float along = dot(rel, -dir);
        float perp = abs(dot(rel, float2(-dir.y, dir.x)));
        if (along >= 0.0 && along < 0.085 && perp < (pixSize * 0.75 / max(Resolution.y, 1.0))) {
            float tailFrac = along / 0.085;
            float fade = (1.0 - tailFrac) * sin(prog * 3.14159265);
            float3 meteorCol = lerp(float3(0.98, 0.99, 1.0), KISEKI_ACCENT_PRIMARY, tailFrac);
            animBg += meteorCol * fade * 0.82 * (1.0 - smoothstep(0.55, 0.70, tex.x));
        }
    }
#endif

    // --- 5. Optional retro CRT scanlines ------------------------------------
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
