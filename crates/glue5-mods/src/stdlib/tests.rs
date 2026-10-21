use mlua::{Lua, LuaOptions, StdLib};

use super::install;

/// A VM with the library over stand-ins for the natives: each native
/// records its call in `calls` and returns 0; `setup` changes them first.
fn stubbed(setup: &str) -> Lua {
    let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).unwrap();
    lua.load(
        r#"
        calls = {}
        local function namespace(ns)
            return setmetatable({}, { __index = function(t, k)
                local f = function(...) table.insert(calls, { ns .. "." .. k, ... }) return 0 end
                rawset(t, k, f)
                return f
            end })
        end
        for _, ns in { "misc", "streaming", "graphics", "player", "entity", "vehicle", "ped", "object",
            "shapetest", "camera", "zone", "path", "hud", "pad" } do
            _G[ns] = namespace(ns)
        end
        local none = function() return {} end
        world = { vehicles = none, peds = none, objects = none, pickups = none }
        ui = { notify = function(text) table.insert(calls, { "app.notify", text }) end }
        wait = function() end
        task = { spawn = function(f, ...) f(...) end, wait = wait }
        mod = {}
        "#,
    )
    .exec()
    .unwrap();
    lua.load(setup).exec().unwrap();
    install(&lua).unwrap();
    lua
}

fn eval<T: mlua::FromLuaMulti>(lua: &Lua, code: &str) -> T {
    lua.load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"))
}

/// The game's own values, from game.wasm and the natives database.
#[test]
fn constants() {
    let lua = stubbed("");
    for (code, value) in [
        ("Control.JUMP", 22),
        ("Control.CONTEXT", 51),
        ("Control.VEH_ACCELERATE", 71),
        ("Bone.R_HAND", 57005),
        ("ExplosionType.GRENADE", 0),
        ("ExplosionType.CAR", 7),
        ("ExplosionType.DONTCARE", -1),
        ("PedConfigFlag.CreatedByFactory", 0),
        ("PedConfigFlag.WillFlyThroughWindscreen", 32),
        ("PedConfigFlag.NoCriticalHits", 2),
        ("HudComponent.WANTED_STARS", 1),
        ("VehicleSeat.DRIVER", -1),
    ] {
        assert_eq!(eval::<i64>(&lua, code), value, "{code}");
    }
}
