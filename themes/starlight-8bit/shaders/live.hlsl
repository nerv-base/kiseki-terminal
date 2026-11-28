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

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float texW, texH;
    shaderTexture.GetDimensions(texW, texH);
    float2 texSize = max(float2(texW, texH), float2(1.0, 1.0));
    float2 uv = saturate(pos.xy / texSize);
    float4 term = shaderTexture.Load(int3(pos.xy, 0));

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgSize = max(float2(imgW, imgH), float2(1.0, 1.0));
    float2 imgUV = kiseki_cover_uv(uv, texSize, imgSize, 0.42);

    float narrow = kiseki_narrow_factor(texSize);
    float bgOpacity = KISEKI_BG_OPACITY * lerp(0.28, 1.0, narrow);
    float3 wall = imageTexture.Sample(samplerState, imgUV).rgb;

    // --- 1. Cozy breathing pulse on glowing cyan monitor & moon highlights --
    float rightZone = smoothstep(0.58, 0.78, imgUV.x) * narrow;
    float cyanGlow = saturate(wall.g * 0.6 + wall.b * 0.6 - wall.r * 0.5);
    float crtPulse = 0.5 + 0.5 * sin(Time * KISEKI_PARAM_CRT_PULSE_HZ);
    wall += wall * cyanGlow * rightZone * (0.14 * crtPulse);

    float3 animBg = lerp(KISEKI_BASE_BG, wall, bgOpacity);

    // --- 2. Crisp 3x3 pixel-grid twinkling stars in upper midnight sky ------
    float dpiScale = max(Scale, 1.0);
    float dipScale = 900.0 * dpiScale;
    float2 dipSize = texSize / dipScale;
    float pixSize = max(2.0, round(3.0 * dpiScale));
    float2 pixCoord = floor(pos.xy / max(pixSize, 1.0));
    float2 cell = floor(pixCoord / 18.0);
    float2 local = pixCoord - cell * 18.0;
    float rnd = kiseki_hash21(cell);

    float skyMask = (1.0 - smoothstep(0.42, 0.58, uv.y)) * (1.0 - smoothstep(0.58, 0.78, uv.x));
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

    // --- 3. Periodic 8-bit diagonal pixel shooting star (DIP-normalized) ----
#if KISEKI_QUALITY_LEVEL >= 2
    float cycle = max(KISEKI_PARAM_METEOR_CYCLE_SEC, 1.0);
    float idx = floor(Time / cycle);
    float tLocal = frac(Time / cycle);
    if (tLocal < 0.24) {
        float prog = tLocal / 0.24;
        float2 startP = float2(
            0.10 + kiseki_hash21(float2(idx, 3.1)) * 0.24,
            0.08 + kiseki_hash21(float2(idx, 9.7)) * 0.16
        ) * dipSize;
        float2 dir = normalize(float2(0.86, 0.51));
        float2 head = startP + dir * (prog * 0.36);

        float2 qDip = (floor(pos.xy / max(pixSize, 1.0)) + 0.5) * pixSize / dipScale;
        float2 rel = qDip - head;
        float along = dot(rel, -dir);
        float perp = abs(dot(rel, float2(-dir.y, dir.x)));
        float halfWidthDip = pixSize * 0.75 / dipScale;
        if (along >= 0.0 && along < 0.095 && perp < halfWidthDip) {
            float tailFrac = along / 0.095;
            float fade = (1.0 - tailFrac) * sin(prog * 3.14159265);
            float3 meteorCol = lerp(float3(0.98, 0.99, 1.0), KISEKI_ACCENT_PRIMARY, tailFrac);
            animBg += meteorCol * fade * 0.82 * (1.0 - smoothstep(0.55, 0.70, uv.x));
        }
    }
#endif

    // --- 4. Exact Porter-Duff Source-Over compositing -----------------------
    float4 bgPremul = float4(saturate(animBg) * KISEKI_WINDOW_ALPHA, KISEKI_WINDOW_ALPHA);
    float4 outColor;
    outColor.rgb = term.rgb + bgPremul.rgb * (1.0 - term.a);
    outColor.a = term.a + bgPremul.a * (1.0 - term.a);
    return saturate(outColor);
}
