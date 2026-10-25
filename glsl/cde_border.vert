#version 450

layout(location = 0) out vec2 outTexCoord;
layout(location = 1) out vec2 outPixelPos;

layout(push_constant) uniform PushConstants {
    mat4 mvp;
    vec2 element_size;
    uint is_focused;
    uint is_dragging;
    uint is_pressed;
    uint title_chars[16];
} push;

// Explicitly define 6 static vertex index combinations matching a standard TRIANGLE_LIST mesh quad
// This maps [0.0 to 1.0] relative position multipliers across 2 distinct triangles on the fly
const vec2 DECK_UVS[6] = vec2[](
    vec2(0.0, 0.0), // Triangle 1: Top-Left
    vec2(1.0, 0.0), // Triangle 1: Top-Right
    vec2(0.0, 1.0), // Triangle 1: Bottom-Left

    vec2(0.0, 1.0), // Triangle 2: Bottom-Left (Re-use)
    vec2(1.0, 0.0), // Triangle 2: Top-Right (Re-use)
    vec2(1.0, 1.0)  // Triangle 2: Bottom-Right
);

void main() {
    // Safely look up the exact vertex coordinates matching your pipeline's primitive topology type
    vec2 unit_pos = DECK_UVS[gl_VertexIndex];

    // Pass UV coordinates directly to the fragment shader
    outTexCoord = unit_pos;

    // Scale local unit box coordinates to the application window size
    outPixelPos = unit_pos * push.element_size;

    // Transform coordinates directly via your top-left relative orthographic matrix projection
    gl_Position = push.mvp * vec4(outPixelPos, 0.0, 1.0);
}
