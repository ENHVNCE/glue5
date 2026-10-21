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
