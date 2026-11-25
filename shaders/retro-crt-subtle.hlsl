// Starlight 8-Bit Subtle CRT Phosphor & Scanline Shader for Windows Terminal
// Designed to preserve razor-sharp pixel typography while adding authentic retro CRT atmosphere.

Texture2D shaderTexture : register(t0);
SamplerState samplerState : register(s0);

cbuffer PixelShaderSettings : register(b0) {
    float  Time;
    float  Scale;
    float2 Resolution;
    float4 Background;
};

// Subtle phosphor bloom on luminous neon pixels (cyan/pink/gold highlights)
float4 SampleBloom(float2 uv, float2 texel) {
    float4 sum = 0;
    sum += shaderTexture.Sample(samplerState, uv + float2(-texel.x, 0)) * 0.20;
    sum += shaderTexture.Sample(samplerState, uv + float2( texel.x, 0)) * 0.20;
    sum += shaderTexture.Sample(samplerState, uv + float2(0, -texel.y)) * 0.20;
    sum += shaderTexture.Sample(samplerState, uv + float2(0,  texel.y)) * 0.20;
    sum += shaderTexture.Sample(samplerState, uv + float2(-texel.x, -texel.y)) * 0.05;
    sum += shaderTexture.Sample(samplerState, uv + float2( texel.x, -texel.y)) * 0.05;
    sum += shaderTexture.Sample(samplerState, uv + float2(-texel.x,  texel.y)) * 0.05;
    sum += shaderTexture.Sample(samplerState, uv + float2( texel.x,  texel.y)) * 0.05;
    return sum;
}

float4 main(float4 pos : SV_POSITION, float2 tex : TEXCOORD) : SV_TARGET {
    float2 texel = 1.0 / Resolution;
    float4 color = shaderTexture.Sample(samplerState, tex);

    // 1. Gentle phosphor bloom only on bright foreground glyphs
    float4 blur = SampleBloom(tex, texel * 1.25);
    float luma = dot(blur.rgb, float3(0.299, 0.587, 0.114));
    float glowMask = smoothstep(0.32, 0.85, luma);
    color.rgb += blur.rgb * glowMask * 0.14;

    // 2. Whisper-soft 2px CRT scanlines (6% modulation - never hurts legibility)
    float scan = sin(pos.y * 3.14159265) * 0.5 + 0.5;
    color.rgb *= lerp(0.94, 1.0, scan);

    // 3. Very subtle edge vignette for cozy CRT depth
    float2 centered = tex * 2.0 - 1.0;
    float vig = 1.0 - dot(centered, centered) * 0.06;
    color.rgb *= saturate(vig);

    return color;
}
