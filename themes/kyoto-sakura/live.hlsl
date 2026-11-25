// ============================================================================
// ★ Kiseki Terminal — Kyoto Sakura Live Fantasy Shader (60fps GPU)
// Drifting & tumbling sakura petals, meandering golden shrine fireflies,
// warm flickering stone lantern glow, and tranquil moonlit pond water ripples.
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

static const float BG_OPACITY = 0.46;

float hash21(float2 p) {
    p = frac(p * float2(127.1, 311.7));
    p += dot(p, p + 34.5);
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

// Drifting & fluttering sakura petal layer (gentle breeze right-to-left & downward)
float SakuraLayer(float2 aspectUV, float gridScale, float speed, float seed) {
    float2 p = aspectUV * gridScale;
    p.x += Time * speed * 0.55;
    p.y -= Time * speed * 0.42;

    float2 id = floor(p);
    float2 f  = frac(p) - 0.5;
    float h = hash21(id + seed);
    if (h < 0.62) return 0.0;

    float phase = h * 31.4159 + Time * (1.2 + h * 1.4);
    float2 offset = float2(sin(phase * 0.7) * 0.24, cos(phase * 0.9) * 0.20);
    float2 q = f - offset;

    // Rotate & flutter-squash to simulate 3D tumbling petal
    float ang = phase * 0.85;
    float ca = cos(ang), sa = sin(ang);
    float2 rq = float2(q.x * ca - q.y * sa, q.x * sa + q.y * ca);
    float flutter = 0.45 + 0.55 * abs(sin(phase * 1.3));
    float d = length(float2(rq.x * 1.2, rq.y * (2.8 / flutter)));
    return smoothstep(0.082, 0.032, d) * (0.55 + 0.45 * h);
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float4 term = shaderTexture.Sample(samplerState, tex);

    float bgAlpha = max(Background.a, 0.001);
    float3 baseBg = Background.rgb / bgAlpha;

    float imgW, imgH;
    imageTexture.GetDimensions(imgW, imgH);
    float2 imgUV = GetBottomRightCoverUV(tex, Resolution, float2(max(imgW, 1.0), max(imgH, 1.0)));

    // 1. Tranquil moonlit temple pond horizontal shimmer in bottom-right reflection
    float pondMask = smoothstep(0.46, 0.62, imgUV.x) * smoothstep(0.76, 0.86, imgUV.y);
    float rippleX  = sin(imgUV.y * 125.0 - Time * 1.8 + imgUV.x * 18.0) * 0.0022 * pondMask;
    float rippleY  = cos(imgUV.x * 70.0  + Time * 1.3) * 0.0009 * pondMask;
    float3 wall = imageTexture.Sample(samplerState, saturate(imgUV + float2(rippleX, rippleY))).rgb;

    // 2. Warm organic stone lantern candle-glow flicker (around lantern center ~0.62, 0.46)
    float lanternDist = length((imgUV - float2(0.615, 0.44)) * float2(1.6, 1.0));
    float lanternHalo = exp(-lanternDist * 6.5);
    float flicker = 0.55 + 0.28 * sin(Time * 2.7) + 0.17 * sin(Time * 6.1 + 1.4);
    wall += float3(1.0, 0.72, 0.34) * lanternHalo * (0.15 * flicker);

    float3 animBg = lerp(baseBg, wall, BG_OPACITY);

    // 3. Two parallax layers of drifting & fluttering sakura petals
    float2 aspectUV = float2(tex.x * (Resolution.x / max(Resolution.y, 1.0)), tex.y);
    float petalsFar  = SakuraLayer(aspectUV, 5.2, 0.28, 3.7);
    float petalsNear = SakuraLayer(aspectUV, 3.4, 0.42, 11.9);

    float3 sakuraPink  = float3(1.00, 0.62, 0.78); // #FF9EC7
    float3 sakuraLight = float3(1.00, 0.86, 0.93);
    animBg += sakuraPink  * (petalsFar  * 0.24);
    animBg += sakuraLight * (petalsNear * 0.32);

    // 4. Meandering golden shrine fireflies (6 soft glowing orbs drifting peacefully)
    float3 fireflyCol = float3(1.0, 0.80, 0.44); // #FFC971 lantern gold
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
        float2 fp = float2((center.x + wander.x) * (Resolution.x / max(Resolution.y, 1.0)),
                           center.y + wander.y);
        float d = length(aspectUV - fp);
        float pulse = pow(0.5 + 0.5 * sin(Time * (1.1 + fi * 0.25) + fi * 3.7), 2.5);
        float orb = exp(-d * 115.0) * 0.55 + exp(-d * 30.0) * 0.14;
        animBg += fireflyCol * orb * pulse;
    }

    // 5. Composite razor-sharp terminal text & HUD over animated background
    float3 diff = abs(term.rgb - Background.rgb);
    float maxDiff = max(diff.r, max(diff.g, diff.b));
    float alphaDiff = max(0.0, term.a - Background.a);
    float fgMask = saturate(max(smoothstep(0.012, 0.055, maxDiff), smoothstep(0.005, 0.035, alphaDiff)));

    float3 outRgb = term.rgb + (animBg - baseBg) * bgAlpha * (1.0 - fgMask);
    return float4(saturate(outRgb), max(term.a, bgAlpha));
}
