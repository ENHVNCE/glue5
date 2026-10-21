-- The mods' standard library (stdlib/*.luau): its additions to the tables declared before.

-- Seats for ped.setPedIntoVehicle and the vehicle natives.
declare VehicleSeat: { ANY_FREE: number, DRIVER: number, PASSENGER: number, LEFT_REAR: number, RIGHT_REAR: number }
-- Fonts for hud.setTextFont.
declare Font: { CHALET_LONDON: number, HOUSE_SCRIPT: number, MONOSPACE: number, CHALET_COMPRIME_COLOGNE: number, PRICEDOWN: number }

-- Rotations as the game has them, in degrees (x pitch, y roll, z yaw). Luau's `vector` library has
-- lerp, angle, dot, cross, normalize.
declare geo: {
	-- Where a rotation points (a unit vector).
	direction: (rot: vector) -> vector,
	-- The rotation (no roll) that points along `dir`.
	rotation: (dir: vector) -> vector,
	-- The direction of a heading, on the ground; and the heading of a direction (0 north, 90 west).
	fromHeading: (heading: number) -> vector,
	heading: (dir: vector) -> number,
	-- The distance between two places (on the ground with `flat`).
	distance: (a: vector, b: vector, flat: boolean?) -> number,
	-- A rotation's quaternion, x, y, z, w (entity.setEntityQuaternion); and back.
	quaternion: (rot: vector) -> (number, number, number, number),
	fromQuaternion: (x: number, y: number, z: number, w: number) -> vector,
}

declare task: typeof(task) & {
	-- Runs `f` every `ms` milliseconds of game time, as a task; the function returned stops it.
	every: <A...>(ms: number, f: (A...) -> ...any, A...) -> () -> (),
}

declare mod: typeof(mod) & {
	-- Runs `f` when the mod stops (stopped, restarted, failed or done), on its script, in one last turn
	-- of 2 ms at most: put back what it changed, delete what it made. It cannot wait.
	onStop: (f: () -> ()) -> (),
}

export type AssetKind = "model" | "anim" | "animSet" | "clipSet" | "ptfx" | "texture"

declare streaming: typeof(streaming) & {
	-- Loads an asset (a model by default: its name or hash), waiting until it is (10 s at most):
	-- false if it is not in the game or did not load in time.
	load: (asset: string | number, kind: AssetKind?, timeoutMs: number?) -> boolean,
}

-- Entities made where the player is (5 m ahead, facing as they do) or at `pos`; their model is
-- loaded (waiting) and released. nil if the model did not load.
declare spawn: {
	vehicle: (model: VehicleModel | Hash, pos: vector?, heading: number?, options: { warpIn: boolean? }?) -> Vehicle?,
	ped: (model: PedModel | Hash, pos: vector?, heading: number?) -> Ped?,
	-- `dynamic`: physics moves it; `ground`: placed on the ground.
	prop: (model: string | Hash, pos: vector?, options: { dynamic: boolean?, ground: boolean? }?) -> Object?,
}
