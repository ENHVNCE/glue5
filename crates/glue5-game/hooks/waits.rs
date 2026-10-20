// Hooks for the runtime's `waits.rs` (see ../hooks.rs).
&[
    // Waits the engine polls with 1 ms sleeps: for the visibility scan's
    // jobs, and for the GPU before a frame's drawing.
    ("poll_event", "rage::sysIpcPollEvent(rage::sysIpcEventTag*)"),
    ("begin_draw", "rage::grcSetup::BeginDraw(bool)"),
    ("is_fence_pending", "rage::grcDevice::IsFencePending(rage::__grcFenceHandle*, unsigned int)"),
]
