//! Fading peds. An entity fading in or out (its alpha below 255: a ped
//! coming or going with the distance, a script's SET_ENTITY_ALPHA) draws
//! stippled, with a technique group forced that clips: "deferredalphaclip"
//! (stipple fades, without MSAA) or a "deferredsubsample*" one (with MSAA).
//! Each shader draws with its technique for that group, and draws nothing
//! when it has none: `grmShader::BeginDraw` gives no passes. This game's
//! `ped.fxc` has neither (its hair shaders have both), so a fading ped was
//! its hair alone.
//!
//! A shader with no technique for the group draws with its "deferred" one
//! instead, stippled as the engine wants it: the stipple mask
//! `CShaderLib::SetStippleAlpha` sets for the entity goes to the renderer
//! as `op::STIPPLE`, the share of each 4x4 block of pixels drawn, and back
//! to none after the draw. Only while the engine stipples: the same groups
//! are forced for draws that are not fading (the cutout pass: hair,
//! leaves...), where such a shader still draws nothing, as the game wants.
//! Nor for a shader made for clipping (hair, with another of the groups):
//! it draws, clipped, where its own group is forced, and a copy with its
//! "deferred" technique would not clip. Shaders that have the group are
//! unchanged.

use std::sync::atomic::{AtomicBool, Ordering};

use glue5_game::{Env, exports, orig};

use crate::gpu::op;
use crate::memory::Mem;

/// The technique group forced on every shader's draws (-1: none), as
/// `grmShader::SetForcedTechniqueGroupId` keeps it.
const FORCED_GROUP: u64 = 6634168;
/// The technique groups' ids, as `DeferredLighting::InitShaders` looks them
/// up: "deferred", then the clipping ones ("deferredalphaclip",
/// "deferredsubsamplealphaclip", "deferredsubsamplealpha").
const DEFERRED_GROUP: u64 = 21490232;
const FADE_GROUPS: [u64; 3] = [7248300, 7248304, 7248320];
/// The stipple mask (u16) of the entity being drawn, a bit for each pixel
/// of a 4x4 block: `CShaderLib::SetStippleAlpha` sets it from the entity's
/// alpha (stipple fades), `CShaderLib::ResetAlpha` sets it back to all
/// (0xffff) after the entity.
const STIPPLE_MASK: u64 = 23984584;

/// The first of the draw types' technique slots (`grmShader::BeginDraw`
/// adds the draw type).
const DRAW_TYPE_BASE: u64 = 12356216;

/// A stipple was sent to the renderer for the draw in progress.
static STIPPLING: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicBool = AtomicBool::new(false);

/// The technique `shader` draws with for technique group `group` and
/// `draw_type` (0: none), as `grmShader::BeginDraw` looks it up in its
/// effect: a byte for each group and draw type.
fn technique_of(m: Mem, shader: u64, draw_type: u32, group: i32) -> u8 {
    let effect = m.u64(shader + 8);
    let slot = i64::from(m.i32(DRAW_TYPE_BASE) + draw_type as i32);
    m.u8(effect.wrapping_add_signed(i64::from(group) * 8 + slot) + 80)
}

/// Sends `op::STIPPLE`: `level` of 16 pixels drawn (0: all).
unsafe fn stipple(env: *mut Env, level: u32) {
    let m = unsafe { Mem::of(env) };
    let Ok(top) = (unsafe { exports::emscripten_stack_get_current(env) }) else { return };
    if let Ok(at) = unsafe { exports::_emscripten_stack_alloc(env, 16) } {
        m.set_u32(at, level);
        unsafe { orig::gpu_emit(env, op::STIPPLE, at, 1) };
    }
    let _ = unsafe { exports::_emscripten_stack_restore(env, top) };
}

/// Hook for `rage::grmShader::BeginDraw(eDrawType, bool, grcEffectTechnique) const`:
/// the passes to draw (0: none).
unsafe extern "C" fn shader_begin_draw(env: *mut Env, shader: u64, draw_type: u32, restore: u32, technique: u32) -> u32 {
    let passes = unsafe { orig::shader_begin_draw(env, shader, draw_type, restore, technique) };
    // An explicit technique, or one found: as it is.
    if passes != 0 || technique != 0 {
        return passes;
    }
    let m = unsafe { Mem::of(env) };
    let forced = m.i32(FORCED_GROUP);
    if forced < 0 || !FADE_GROUPS.iter().any(|&g| m.i32(g) == forced) {
        return passes;
    }
    // A shader made for clipping (hair: it has another of the groups) draws
    // in the pass that forces its own; a copy here would not clip.
    if FADE_GROUPS.iter().any(|&g| technique_of(m, shader, draw_type, m.i32(g)) != 0) {
        return passes;
    }
    // Of 16 pixels; all of them is not a fading entity, none draws nothing.
    let level = m.u16(STIPPLE_MASK).count_ones();
    if level == 0 || level >= 16 {
        return passes;
    }
    m.set_i32(FORCED_GROUP, m.i32(DEFERRED_GROUP));
    let passes = unsafe { orig::shader_begin_draw(env, shader, draw_type, restore, technique) };
    m.set_i32(FORCED_GROUP, forced);
    if passes != 0 {
        unsafe { stipple(env, level) };
        STIPPLING.store(true, Ordering::Relaxed);
        if !LOGGED.swap(true, Ordering::Relaxed) {
            log::info!("fades: a shader with no fade technique draws with its deferred one, stippled (fading peds)");
        }
    }
    passes
}

/// Hook for `rage::grmShader::EndDraw() const` (`this` is gone from the
/// build: it ends the draw in progress): the stipple ends with the draw.
unsafe extern "C" fn shader_end_draw(env: *mut Env) {
    unsafe { orig::shader_end_draw(env) };
    if STIPPLING.swap(false, Ordering::Relaxed) {
        unsafe { stipple(env, 0) };
    }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut glue5_game::Hooks) {
    hooks.shader_begin_draw = Some(shader_begin_draw);
    hooks.shader_end_draw = Some(shader_end_draw);
}
