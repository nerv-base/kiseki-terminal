// ============================================================================
// ★ Kiseki Terminal — Starlight 8-Bit Live Pixel Shader (60fps GPU)
// Real-time pixel-quantized twinkling stars, periodic diagonal shooting stars,
// shimmering pixel ocean waves, and cozy CRT/moonlight pulse — zero text blur.
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

static const float BG_OPACITY = 0.42;

float hash21(float2 p) {
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

// Aspect-preserving UniformToFill + BottomRight alignment matching Windows Terminal
float2 GetBottomRightCoverUV(float2 uv, float2 screenSize, float2 imgSize) {
    float screenAspect = screenSize.x / max(screenSize.y, 1.0);
    float imgAspect    = imgSize.x / max(imgSize.y, 1.0);
    float2 outUV = uv;
    if (screenAspect > imgAspect) {
        // Screen wider than image -> crop top, keep bottom anchored
        float visH = imgAspect / screenAspect;
        outUV.y = (1.0 - visH) + uv.y * visH;
    } else {
        // Screen taller/narrower -> crop left, keep right anchored
        float visW = screenAspect / imgAspect;
        outUV.x = (1.0 - visW) + uv.x * visW;
    }
    return saturate(outUV);
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);

    // 1. Extract straight base background color
    float bgAlpha = max(Background.a, 0.001);
    float3 baseBg = Background.rgb / bgAlpha;

    // 2. Compute exact BottomRight UniformToFill UV for t1 wallpaper
    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgUV = GetBottomRightCoverUV(tex, Resolution, float2(max(imgW, 1.0), max(imgH, 1.0)));

    // 3. Subtle pixel-stepped ocean wave shimmer (mid-lower sea reflection band)
    float oceanMask = smoothstep(0.54, 0.61, imgUV.y) * (1.0 - smoothstep(0.76, 0.85, imgUV.y))
                    * smoothstep(0.32, 0.48, imgUV.x) * (1.0 - smoothstep(0.76, 0.86, imgUV.x));
    float pixRow = floor(pos.y / 3.0);
    float waveShift = sin(pixRow * 0.65 + Time * 2.2) * 0.0022 * oceanMask;
    float2 warpedUV = saturate(imgUV + float2(waveShift, 0.0));

    float3 wall = imageTexture.Sample(samplerState, warpedUV).rgb;

    // 4. Cozy breathing pulse on glowing cyan CRT monitor & mint moon highlights on right
    float rightZone = smoothstep(0.58, 0.78, imgUV.x);
    float cyanGlow  = saturate(wall.g * 0.6 + wall.b * 0.6 - wall.r * 0.5);
    float crtPulse  = 0.5 + 0.5 * sin(Time * 2.1);
    wall += wall * cyanGlow * rightZone * (0.12 * crtPulse);

    // Blend wallpaper into theme midnight base
    float3 animBg = lerp(baseBg, wall, BG_OPACITY);

    // 5. Crisp 3x3 pixel-grid twinkling stars in upper midnight sky (left & mid sky)
    float pixSize = max(2.0, round(3.0 * Scale));
    float2 pixCoord = floor(pos.xy / pixSize);
    float2 cell = floor(pixCoord / 18.0);
    float2 local = pixCoord - cell * 18.0;
    float rnd = hash21(cell);

    float skyMask = (1.0 - smoothstep(0.42, 0.58, tex.y)) * (1.0 - smoothstep(0.58, 0.78, tex.x));
    if (rnd > 0.74 && skyMask > 0.01) {
        float2 starPos = floor(float2(hash21(cell + 17.3), hash21(cell + 53.7)) * 14.0) + 2.0;
        float d = max(abs(local.x - starPos.x), abs(local.y - starPos.y));
        if (d < 0.6) {
            float twinkle = pow(0.5 + 0.5 * sin(Time * (1.4 + rnd * 2.6) + rnd * 40.0), 3.0);
            float3 starCol = (rnd > 0.92) ? float3(1.0, 0.51, 0.72) :
                             (rnd > 0.83) ? float3(0.40, 0.94, 1.0) :
                                            float3(0.95, 0.97, 1.0);
            animBg += starCol * twinkle * 0.42 * skyMask;
        }
    }

    // 6. Periodic 8-bit diagonal pixel shooting star across the quiet left night sky
    float cycle = 6.0;
    float idx = floor(Time / cycle);
    float tLocal = frac(Time / cycle);
    if (tLocal < 0.24) {
        float prog = tLocal / 0.24;
        float2 startP = float2(0.10 + hash21(float2(idx, 3.1)) * 0.24,
                               0.08 + hash21(float2(idx, 9.7)) * 0.16);
        float2 dir = normalize(float2(0.86, 0.51));
        float2 head = startP + dir * (prog * 0.34);

        // Snap current sample to coarse pixel grid for authentic 8-bit stair-step look
        float2 qUV = (floor(pos.xy / pixSize) + 0.5) * pixSize / Resolution;
        float2 rel = qUV - head;
        float along = dot(rel, -dir);
        float perp  = abs(dot(rel, float2(-dir.y, dir.x)));
        float tailLen = 0.085;
        if (along >= 0.0 && along < tailLen && perp < (pixSize * 0.75 / Resolution.y)) {
            float fade = (1.0 - along / tailLen) * sin(prog * 3.14159265);
            float3 meteorCol = lerp(float3(0.98, 0.99, 1.0), float3(0.40, 0.94, 1.0), along / tailLen);
            animBg += meteorCol * fade * 0.78 * (1.0 - smoothstep(0.55, 0.70, tex.x));
        }
    }

    // 7. Composite razor-sharp terminal content & HUD over animated background (premultiplied alpha safe)
    float3 diff = abs(term.rgb - Background.rgb);
    float maxDiff = max(diff.r, max(diff.g, diff.b));
    float alphaDiff = max(0.0, term.a - Background.a);
    float fgMask = saturate(max(smoothstep(0.012, 0.055, maxDiff), smoothstep(0.005, 0.035, alphaDiff)));

    float3 outRgb = term.rgb + (animBg - baseBg) * bgAlpha * (1.0 - fgMask);
    return float4(saturate(outRgb), max(term.a, bgAlpha));
}
