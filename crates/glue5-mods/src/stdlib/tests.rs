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

/// `code` fails with an error that says `error`.
fn fails(lua: &Lua, code: &str, error: &str) {
    let e = lua.load(code).exec().unwrap_err().to_string();
    assert!(e.contains(error), "{code}: {e}");
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

#[test]
fn every() {
    let lua = stubbed("");
    fails(&lua, "task.every('soon', print)", "task.every: the interval");
    fails(&lua, "task.every(100)", "task.every: the second argument");
}

#[test]
fn rotations() {
    let lua = stubbed("");
    let close = |code: &str| {
        let ok: bool = eval(&lua, code);
        assert!(ok, "{code}");
    };
    close("return vector.magnitude(geo.direction(vector.create(0, 0, 90)) - vector.create(-1, 0, 0)) < 1e-6");
    close("return math.abs(geo.heading(geo.fromHeading(135)) - 135) < 1e-4");
    close("local d = geo.direction(vector.create(20, 0, 30)) return vector.magnitude(geo.direction(geo.rotation(d)) - d) < 1e-5");
    close(
        "local r = vector.create(20, 10, 30)
         return vector.magnitude(geo.fromQuaternion(geo.quaternion(r)) - r) < 1e-3",
    );
    // The quaternion turns the forward axis where the rotation points.
    close(
        "local r = vector.create(25, 0, -60)
         local x, y, z, w = geo.quaternion(r)
         -- (0, 1, 0) rotated by q: the second column of its matrix.
         local f = vector.create(2 * (x * y - w * z), 1 - 2 * (x * x + z * z), 2 * (y * z + w * x))
         return vector.magnitude(f - geo.direction(r)) < 1e-5",
    );
    close("return geo.distance(vector.create(0, 0, 0), vector.create(3, 4, 12), true) == 5");
}
