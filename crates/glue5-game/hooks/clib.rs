// Hooks for the runtime's `clib.rs` (see ../hooks.rs).
&[
    // The C library's decimal parser (the game data's numbers at boot,
    // Scaleform's string-to-number): a native one for plain decimals.
    ("strtod", "strtod"),
]
