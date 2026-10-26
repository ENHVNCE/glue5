// Hooks for the runtime's `fades.rs` (see ../hooks.rs).
&[
    // Every draw of a shader begins here, with its technique for the
    // technique group forced at the time (fading entities and the cutout
    // pass force a clipping one): no technique, nothing drawn.
    ("shader_begin_draw", "rage::grmShader::BeginDraw(rage::grmShader::eDrawType, bool, rage::grcEffectTechnique__) const"),
    ("shader_end_draw", "rage::grmShader::EndDraw() const"),
]
