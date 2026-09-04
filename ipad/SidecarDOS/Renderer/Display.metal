#include <metal_stdlib>
using namespace metal;
struct Vertex { float4 position [[position]]; float2 uv; };
vertex Vertex displayVertex(uint id [[vertex_id]]) {
    const float2 positions[] = {float2(-1,1), float2(-1,-1), float2(1,1), float2(1,-1)};
    const float2 coordinates[] = {float2(0,0), float2(0,1), float2(1,0), float2(1,1)};
    return {float4(positions[id],0,1), coordinates[id]};
}
fragment float4 displayFragment(Vertex v [[stage_in]], texture2d<float> y [[texture(0)]], texture2d<float> uv [[texture(1)]]) {
    constexpr sampler linearSampler(filter::linear, address::clamp_to_edge);
    float luma = (y.sample(linearSampler,v.uv).r - 16.0/255.0) * (255.0/219.0);
    float2 chroma = (uv.sample(linearSampler,v.uv).rg - 128.0/255.0) * (255.0/224.0);
    float3 rgb = float3(luma + 1.5748*chroma.y, luma - 0.187324*chroma.x - 0.468124*chroma.y, luma + 1.8556*chroma.x);
    return float4(saturate(rgb),1);
}
