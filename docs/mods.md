# Mods and GLUE 5 Studio

Mods are small [Luau](https://luau.org) scripts that run inside the game,
as the game's own scripts do, and call its native functions: spawn a car,
change the weather, add a menu. **GLUE 5 Studio** is the editor for them,
served by the app to your browser, on the same computer or from your phone
to your computer.

<p align="center">
  <img src="images/studio.webp" alt="GLUE 5 Studio: a mod open in the editor, its menu live from the game below" width="100%">
</p>

## Using mods

1. Turn mods on: the **Mods** page (start page, Settings › Game › Mods, or
   F7 in the game), its **Mods** switch.
2. Put mods in the mods folder, one folder each, or import a `.zip` in the
   Studio. The folder is `mods` in the [user folder](getting-started.md#where-your-data-is)
   on computers, and next to the `game` folder on phones and tablets
   (`Documents/mods` on iPhone, where Files and the Finder reach it).
3. Start a game. Mods marked to start with the game run once you are in;
   the others start from the Mods page or the Studio.

Mods with a menu show it on the Mods page's **MENUS** (F7, or the button
next to ⚙ on touch screens), drawn by the app: it works with touch,
controllers, keyboard and mouse.

## GLUE 5 Studio

The Mods page's **Studio** tab starts it and shows its address, its
pairing code and a QR code. Open it in a browser on the same device, or
turn on **Other devices** and open the address on another one, a computer
or a tablet (phones default to it: their screens are no place to write
code).

<p align="center">
  <img src="images/studio-phone.jpg" alt="The Studio on a phone: the mods' menus" width="320">
</p>

- **An editor that knows the game.** Every native is typed and documented,
  with completions, hovers and signatures as you type. Model names
  (vehicles, peds, weapons) come from your installed game, so a misspelt
  car is an error before it runs.
- **Live.** Saving restarts the mod in the running game. The console runs
  Luau in the game as you type it; each mod's log and errors come back
  with the line they came from.
- **Menus.** The mods' menus, mirrored from the game: use them from the
  Studio, your phone, or the game itself.
- **Mod pages.** Name, description, version, whether it starts with the
  game; export a mod as a `.zip`, import one.
- **Safe to connect.** Only a browser with the pairing code gets in, and
  only from the Studio's own page. A new code (Mods page) closes every
  connection made with the old one.

## Your first mod

A mod is a folder with a `main.luau`, and a `mod.json` that describes it.
This one adds a menu that delivers a car in the colour and with the plate
you choose, and a submenu for the weather and the hour:

```lua
--!strict
-- Garage: any car in any colour, any weather, any hour, from the Mods menu (F7).

local cars = { "adder", "zentorno", "t20", "sanchez", "deluxo" }
local weathers = { "EXTRASUNNY", "CLEAR", "CLOUDS", "RAIN", "THUNDER", "FOGGY", "XMAS" }

local garage = ui.menu("Garage", "Any car, delivered")
local car = garage:choice("Car", cars, "zentorno")
local paint = garage:color("Paint", "#d01818")
local plate = garage:text("Plate", "", { placeholder = "GLUE 5", max = 8 })

local function deliver()
	local model = car.option
	local hash = joaat(model)
	streaming.requestModel(hash)
	while not streaming.hasModelLoaded(hash) do
		wait()
	end
	local me = player.playerPedId()
	local spawned = vehicle.createVehicle(hash, entity.getEntityCoords(me, true), entity.getEntityHeading(me), false, false, false)
	streaming.setModelAsNoLongerNeeded(hash)
	if spawned then
		local c = paint.value
		vehicle.setVehicleCustomPrimaryColour(spawned, c.r, c.g, c.b)
		if plate.value ~= "" then
			vehicle.setVehicleNumberPlateText(spawned, plate.value)
		end
		ped.setPedIntoVehicle(me, spawned, -1)
		ui.notify(`Your {model} is here`)
	end
end

garage:button("Deliver", { note = "Right where you stand", action = "Deliver" }, deliver)

local sky = garage:submenu("Weather & time")
sky:choice("Weather", weathers, 1, function(_, weather)
	misc.setWeatherTypeNowPersist(weather)
end)
local hour = sky:slider("Hour", 19, 0, 23, 1, function(h)
	clock.setClockTime(h, clock.getClockMinutes(), 0)
end)
local minute = sky:number("Minute", 0, { min = 0, max = 59, step = 5 }, function(m)
	clock.setClockTime(clock.getClockHours(), m, 0)
end)
sky:toggle("Freeze time", false, "The clock stops where it is", function(on)
	clock.pauseClock(on)
end)
-- The game's clock runs on: show where it is whenever the page opens.
sky:onOpen(function()
	hour.value = clock.getClockHours()
	minute.value = clock.getClockMinutes()
end)
```

```json
{
  "name": "Garage",
  "description": "Any car in any colour, any weather, any hour: a menu for the open world.",
  "version": "1.1",
  "autostart": true
}
```

It is in [examples/garage](examples/garage), next to
[examples/menu-gallery](examples/menu-gallery), which shows every kind of
menu item: copy a folder into the mods folder to try it.

## What a mod can use

| | |
|---|---|
| **Natives** | The game's script functions by namespace, named the Luau way: `vehicle.createVehicle(...)`, `player.playerPedId()`; and by their game names, the same functions: `VEHICLE.CREATE_VEHICLE(...)`. Outputs come back as extra return values; a handle of 0 as `nil`, and `nil` (or 0) where a native takes none. A vector also goes as three numbers. |
| `native.invoke(hash, ...)` | A native by its ScriptHookV hash or its game name, as ScriptHookV calls it: `native.invoke("0xAF35D0D2583051B0", model, x, y, z, heading, false, false, false)`. See [Porting ScriptHookV mods](#porting-scripthookv-mods). |
| `globals` | The game scripts' globals by ScriptHookV's index: `globals.int(i)`, `float`, `bool`, `string`, `setInt(i, v)`..., `valid(i)`. |
| `memory` | The game's memory: `memory.entity(handle)` (an entity's address), `readFloat(address)`, `writeInt(address, v)`... (U8, U16, U32, Int, U64, Ptr, Float, Vector3, String), `read(address, bytes)` into a Luau `buffer` and `write(address, buffer)`, scratch memory for natives: `alloc(bytes)`, `free(address)`. |
| `memory.offsets` | Where fields of the game's objects are, for this game build: `memory.readFloat(memory.entity(car) + memory.offsets.vehicle.rpm)`. What ScriptHookVDotNet reads from memory: a vehicle's gears, revs, clutch, throttle, brake, steering, fuel, oil, engine temperature and wheel speed; its wheels' tyre and suspension health, tyre temperature and wear, spin; a ped's armour, injury thresholds, money, seeing and hearing ranges; who damaged an entity. The editor's hovers say what each one holds and how (float, byte, pointer). |
| `handling` | A vehicle's handling, every field by handling.meta's name, as the game's own parser describes it (sub-handlings too, `CCarHandlingData`'s, `CBikeHandlingData`'s...): `handling.get(car, "fInitialDriveForce")`, `handling.set("ADDER", "fBrakeForce", 1.2)`, `handling.fields(car)`. Vehicles of a model share it, so a change reaches all of them; values are in the game's units (m/s, radians), and a vehicle's gearbox is set up from them when it is made. |
| `game.version()` | The game build the natives, globals and offsets are for (`"8b0b5899ed"`). |
| `wait(ms)` | Back to the game for `ms` milliseconds (none: until the next frame). |
| `ui.menu(title)` | A menu on the Mods page, with buttons, toggles, sliders, numbers, choices, text, colours and submenus: see [Menus](#menus). `ui.notify(text)` shows a notification. |
| `task.spawn`, `task.delay` | Coroutines on the game's clock (they stop while the game is paused). |
| `input` | The keyboard, the mouse and the player's pad as the game gets them: `input.isDown("F5")`, `input.pressed("PadSouth")` (since the mod's last turn), `input.onKey(function(key, down, mods) ... end)`, `input.mousePosition()`. Keys are named as on a US keyboard by position: `"A"`, `"F5"`, `"LeftShift"`, `"MouseLeft"`. |
| `world` | The vehicles, peds, objects and pickups around and the projectiles in flight, as handles: `world.vehicles()`, or only some, the closest first: `world.peds({ near = pos, radius = 50, model = "a_c_cat_01", max = 5 })`. And the installed game's model names. |
| `events` | The game's script events of this frame, by group (`"ai"`, `"network"`, `"errors"`): `events.count("network")`, `events.at("network", i)` (its name, `"EVENT_NETWORK_DAMAGE_ENTITY"`), `events.data("network", i)`, the data as a table (`d.victim`, `d.damage`, `d.weapon`...) for the common types: entity damaged or destroyed, damage, dead ped seen, pickups collected, weapon dropped, vehicle undrivable, vehicle entered. |
| Structures | Natives that fill a structure or an array give a table or a list: `extrametadata.getDlcVehicleData(i)` (`lockHash`, `model`, `cost`), `getDlcWeaponData`, `getDlcWeaponComponentData` (and their `Sp` ones; text labels as strings), `ped.getPedNearbyVehicles(ped, max)`, `ped.getPedNearbyPeds(ped, ignore, max)`. The same natives raw, ScriptHookV's way: `native.invoke` with `memory.alloc`. |
| `draw` | Images from the mod's folder over the game: `local logo = draw.loadImage("images/logo.png")` (PNG, waits while it is decoded), then each frame `draw.image(logo, 0.9, 0.1, 0.1, nil, { rotation = 15, color = { 255, 255, 255, 200 }, z = 1 })`; `draw.rect` and `draw.text` too, above the images or below. Centred positions and sizes in fractions of the screen, as DRAW_SPRITE's; drawn at the window's resolution over everything the game draws. A mod holds 64 images, 32 MB of them, at most (2048×2048 each), freed when it stops or by `image:release()`. |
| `json`, `storage` | JSON text, and values kept between sessions (1 MB a mod, for this player). |
| `db` | The mod's SQLite database: `db.exec("CREATE TABLE spots (name TEXT, x REAL)")`, `db.exec("INSERT INTO spots VALUES (?, ?)", name, x)`, `db.query("SELECT * FROM spots WHERE x > :x", { x = 10 })` (its rows as tables), `db.transaction(function() ... end)` (undone if it fails). One file for this player, as the storage, 256 MB at most. Queries run on the mod's turn: large work goes in a transaction (300 inserts in one take under a millisecond). |
| `net.request(url)`, `net.socket(url)` | HTTP and HTTPS, as [Lune](https://lune-org.github.io/docs)'s `@lune/net`: `net.request("https://...")` or `net.request({ url = ..., method = "POST", headers = {...}, body = ..., query = {...}, options = { decompress = false } })` gives `{ ok, statusCode, statusMessage, headers, body }`. It waits, as `wait()` does, while the request goes on away from the game. No answer (no network, a bad address, 30 s) is an error. 4 requests at once a mod, answers of 16 MB; gzip and deflate answers are unpacked unless `decompress = false`. `net.urlEncode(s)` and `net.urlDecode(s)` (`binary` as a second argument for bytes that are not text). WebSockets too, as Lune's: `local s = net.socket("wss://...")`, `s:send(text)` (`s:send(data, true)` binary), `s:next()` (waits for a message; nil once closed), `s:close()`, `s.closeCode`; 4 open at once a mod, messages of 16 MB. |
| `mod.settings`, `mod.read` | The mod's own folder. `local s = mod.settings({ key = "F5", speed = 2 })`: the defaults with the values of the folder's `settings.json` over them, the file written with the defaults the first time for the player to edit (ScriptHookV mods' INI file). `mod.read("data/cars.json")` (nil when there is none), `mod.write(path, text)` (a string or a `buffer`), `mod.list("data")`. 16 MB a file, 256 MB the folder; read and written on the mod's turn. |

Mods reach the network through `net`, keep data in `storage` and `db`, and
read and write the files of their own folder; other files are out of their
reach.

Keys reach a mod when they reach the game: not while a page of the app is
open (the settings, the Mods page and its menus), not while the game's
text box takes typed text, and F1, F7 and Home are the app's. The game
sees them too: a mod that takes over one of the game's controls disables
it each frame, as the game's scripts do
(`pad.disableControlAction(0, control, true)`). On touch screens the touch
controls are the player's pad, so a mod for every device puts its
actions in a menu.

**A mod cannot take the game down** through the natives' namespaces. An
error stops that mod with its message and traceback; arguments are checked
before a native runs; a mod gets at most 2 ms a frame, and one that runs 2
seconds without waiting is stopped. `native.invoke`, `globals` and
`memory` are the exception: they do what ScriptHookV does, and can (see
below).

## Standard library

What ScriptHookVDotNet's classes give C# mods, in Luau, in every mod:
typed in the editor like the natives. Functions that wait for the game
(loading, a raycast, the keyboard) wait as `wait()` does.

| | |
|---|---|
| Constants | The game's own tables, by their names in the game without the prefix: `Control.JUMP` (`INPUT_JUMP`, 22), `Bone.R_HAND` (`BONETAG_R_HAND`), `ExplosionType.GRENADE`, `PedConfigFlag.NoCriticalHits`, read from this game build; `HudComponent.WANTED_STARS` (from the natives' notes); `VehicleSeat.DRIVER`, `Font.PRICEDOWN`. Models and weapons go by name: `"adder"`, `"WEAPON_PISTOL"`. |
| `streaming.load(asset, kind?, ms?)` | Loads a model (the default), `"anim"`, `"animSet"`, `"clipSet"`, `"ptfx"` or `"texture"` dictionary, waiting up to `ms` (10 s): `false` if it is not in the game or did not load. |
| `spawn` | `spawn.vehicle("adder", pos?, heading?, { warpIn = true })`, `spawn.ped(model, pos?, heading?)`, `spawn.prop(model, pos?, { dynamic, ground })`: 5 m ahead of the player unless `pos` says, the model loaded and released; `nil` if it did not load. |
| `world` | `world.nearby("peds", me, 30)` (a place or an entity, left out; the closest first), `world.closest("vehicles", me)`, `world.raycast(from, to, { flags, ignore })` → `{ pos, normal, entity, material }` or `nil`, `world.crosshair()` (what the camera looks at), `world.groundZ(pos)`, `world.waypoint()`, `world.setWaypoint(pos)`, `world.zoneName(pos)`, `world.streetName(pos)`. |
| `ui` | The game's own text: `ui.notify(text, { icon = "CHAR_LESTER", title, subtitle })` in the feed (plain `ui.notify(text)` stays the app's), `ui.subtitle(text, ms?)`, `ui.help(text)` (`~INPUT_CONTEXT~` shows the button; each frame it stays), `ui.loading(text)` / `ui.loading()`, `ui.hideHud(HudComponent.CASH)` (this frame), `ui.prompt(title?, default?, max?)` (the game's keyboard; `nil` if cancelled), `ui.buttons({ { control = Control.JUMP, label = "Jump" } })` (instructional buttons until changed, `nil` hides them). |
| `scaleform.load(name)` | A scaleform movie: `sf:call("SHOW_SHARD_WASTED_MP_MESSAGE", "Wasted", "", 5)`, then each frame `sf:draw()` (or `sf:draw(x, y, w, h)`, `sf:draw3d(pos, rot, scale)`); `sf:release()`. |
| `task.every(ms, f)` | `f` every `ms` of game time; the function it returns stops it. |
| `mod.onStop(f)` | `f` runs when the mod stops (stopped, restarted on save, failed or done), on its script, in one last turn of 2 ms: delete what it made, put back what it changed. It cannot wait. Not when the game ends the mod's script itself. |
| `geo` | Rotations as the game has them (degrees: pitch, roll, yaw): `geo.direction(rot)`, `geo.rotation(dir)`, `geo.fromHeading(h)`, `geo.heading(dir)`, `geo.distance(a, b, flat?)`, `geo.quaternion(rot)`, `geo.fromQuaternion(x, y, z, w)`. Luau's `vector` has `lerp`, `angle`, `dot`, `cross`. |

## Menus

`ui.menu(title)` makes a menu, and its methods add the items to it, in
order. Each returns the item: read and set its properties, and the app,
the Studio and a coding agent show the change.

```lua
local trainer = ui.menu("Trainer", "Health, money, the clock")
local god = trainer:toggle("God mode", false, function(on)
	player.setPlayerInvincible(player.playerId(), on)
end)
trainer:number("Cash", 1000, { min = 0, step = 500, unit = "$" }, function(amount)
	-- ...
end)
god.value = true          -- the switch moves; no callback for the mod's own change
god.note = "No damage"    -- the line under the label
```

| Item | Its `value` | The player sees | For example |
|---|---|---|---|
| `button` | | a row with a pill, "Run" or its `action` | `m:button("Deliver", { action = "Go" }, function() ... end)` |
| `toggle` | `true` or `false` | a switch | `m:toggle("God mode", false, function(on) ... end)` |
| `slider` | a number from `min` to `max` | a rail and the value, with its `unit` (`"%"` shows 0–1 as 0–100 %) | `m:slider("Hour", 19, 0, 23, 1, function(hour) ... end)` |
| `number` | a number, `min` and `max` if given | `‹ 1000 ›`, or typed | `m:number("Cash", 1000, { min = 0, step = 100, unit = "$" }, f)` |
| `choice` | the option's index, from 1 (`.option` is its text) | `‹ adder ›`, and the whole list, with a search when it is long | `m:choice("Car", { "adder", "t20" }, "t20", function(i, name) ... end)` |
| `multi` | the chosen indices | the chosen ones, and the list with checks, All and None | `m:multi("Weather", weathers, { 1, 3 }, function(indices, names) ... end)` |
| `text` | a string, up to `max` (64) characters | a field; the system keyboard on touch screens | `m:text("Plate", "", { placeholder = "GLUE 5", max = 8 }, f)` |
| `color` | `{ r, g, b, a }`, 0 to 255 (or `"#rrggbb"` to set it) | a swatch, and a page with a palette, hue, saturation, brightness (opacity with `alpha`) and the hex code | `m:color("Paint", "#d01818", { alpha = true }, f)` |
| `label` | | muted text | `m:label("Three cars delivered")` |
| `separator` | | a heading over the items after it, or a gap | `m:separator("Vehicles")` |
| `progress` | 0 to 1 | a bar with its percentage | `m:progress("Collected", 0.4)` |
| `submenu` | | a row with ›, which opens a menu of its own | `local sky = m:submenu("Weather & time")` |

- **After the value come the options**: a string is the note (the line
  under the label); a table can hold the note, `enabled = false` (shown
  dimmed, not usable), `visible = false` (not shown) and the item's own
  options (`step`, `unit`, `min`, `max`, `placeholder`, `alpha`,
  `action`). Then the callback.
- **Properties.** Every item has `label`, `note`, `enabled`, `visible` and
  `:remove()`, and the ones of its kind: `value`, `options`, `min`,
  `max`... `item:set(v)` is `item.value = v`. A mistyped name is an error
  (`Toggle has no field 'valeu'`).
- **Callbacks** run when a player changes the item, not when the mod does,
  as tasks: they can call natives and `wait`. `value` changes with them:
  a dragged slider gives one callback a frame, with its latest value.
  A text's callback runs when the player is done typing (Enter, Done).
- **Menus** have `title`, `note` (under the title in the list of menus),
  `enabled` and `visible` (a submenu's row), and:
  - `:clear()` removes the items, and the submenus they open, to build the
    menu again;
  - `:onOpen(f)` and `:onClose(f)` run `f` when a player starts and stops
    looking at the menu (on the Mods page, in the Studio, by a coding
    agent): to fill a list when it is needed;
  - `:show()` opens the Mods page at this menu, for a key of the mod's
    (not over another page, nor while text is typed);
  - `:remove()`.

The Mods page's MENUS lists the menus, each with its mod's name. Tap one
(or A, or Enter) to open it; a submenu, a choice's list or a colour opens
over it, and back (‹, B, Esc) goes up one. The page's X closes it where
it is: F7 opens it there again. A controller's focus skips what is
disabled. The Studio shows the same menus, each in a card with the same
way in and back.

Limits, for every run of a mod:

| What | At most |
|---|---|
| Menus and items | 4096, all together, nested at any depth (more: an error) |
| Options of choices and multis | 65536, all together (more: an error) |
| A menu's title, an item's label | 40, 60 characters (longer: cut) |
| A note | 200 characters (cut) |
| An option | 40 characters (cut) |
| A text's value | `max`, 256 at most (cut) |
| A button's action, a unit | 16, 8 characters (cut) |
| Numbers | finite (NaN or infinity: an error) |
| `menu:show()` | once in 2 seconds (more: ignored) |
| What the player does before the mod's next turn | the last value of each item, and 64 presses (the oldest go) |

Menus do not go over the game (the Mods page hides the touch controls),
and have no icons, images or drawing of their own (that is `draw`'s), nor
text over more than one line. Items keep their order: to change it, clear
the menu and build it again.

## Porting ScriptHookV mods

ScriptHookV's and ScriptHookVDotNet's calls have their counterparts:

| ScriptHookV | Here |
|---|---|
| `VEHICLE::CREATE_VEHICLE(...)`, `Function.Call(Hash.CREATE_VEHICLE, ...)` | `VEHICLE.CREATE_VEHICLE(...)` (typed, vectors as vectors and outputs as results), or `native.invoke("CREATE_VEHICLE", ...)` |
| `nativeInit(0xAF35D0D2583051B0); nativePush(...); nativeCall()` | `native.invoke("0xAF35D0D2583051B0", ...)` |
| `getGlobalPtr(i)` | `globals.int(i)`, `globals.setFloat(i, v)`... |
| `getScriptHandleBaseAddress(handle)` | `memory.entity(handle)` |
| `getGameVersion()` | `game.version()` |

- **Hashes** are the natives database's (nativedb), the ones ScriptHookV
  mods use; the game names too. Write a hash as a string,
  `"0xAF35D0D2583051B0"`: as a number it works, but past 2^53 Luau rounds
  it (the editor warns).
- **`native.invoke`** passes the arguments as ScriptHookV does: a
  `Vector3` as three numbers, a pointer as an address (`memory.alloc`) or a
  Luau `buffer` (copied in before the call and back after it, 16 KB at
  most); the native's result comes back. It also reaches the natives the
  namespaces leave out (structures and buffers: `COPY_SCRIPT_STRUCT`,
  `GET_EVENT_DATA`, the save natives), but not `WAIT` (use `wait()`).
- **Globals' indices depend on the game's scripts**: an index from a
  mod for another game version reads something else. Check each one
  against this game. Reads and writes are kept inside the blocks of
  globals the game has loaded.
- **Addresses** are the game's own (offsets in its memory); a pointer read
  from the game's objects (`memory.readPtr`) is an address too. Field
  offsets are not PC GTA V's (another build, another layout):
  ScriptHookVDotNet's offsets do not carry over. `memory.offsets` has
  this build's for what ScriptHookVDotNet reads (`Vehicle.CurrentRPM` is
  `memory.offsets.vehicle.rpm`), and `handling` every handling field by
  name; they hold for this game build only.
- **What can crash the game.** Every address is checked against the
  game's memory, so nothing outside it is touched; inside it nothing is
  protected. A wrong write with `memory` or `globals`, a native given a
  buffer too small for what it writes, or a native that keeps a pointer
  (`START_SAVE_*`, `NETWORK_REGISTER_*_BROADCAST_VARIABLES`) given memory
  that is freed (by `memory.free`, or when the mod stops) can corrupt the
  game or make it stop. Installing such a mod is trusting its author.

## Coding agents

The Studio is also an [MCP](https://modelcontextprotocol.io) server: a
coding agent can write a mod and get its type errors, start it and read
its log, run Luau in the game, search the natives and use the mods' menus.
The Studio's welcome page shows the command for your game, for example:

```sh
claude mcp add --transport http glue5 http://<address>:47110/mcp --header "Authorization: Bearer <code>"
```

How mods run inside the engine is in the [design notes](design.md#mods).
