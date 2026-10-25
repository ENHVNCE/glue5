--!strict
-- What the features share about the game: the player, their vehicle, the game's own texts,
-- moving the player, telling them what happened.

local prefs = require("./prefs")

local M = {}

function M.me(): Ped
	return player.playerPedId()
end

function M.player(): Player
	return player.playerId()
end

-- The vehicle the player is in (any seat), nil on foot.
function M.vehicle(): Vehicle?
	local me = M.me()
	if not ped.isPedInAnyVehicle(me, false) then
		return nil
	end
	return ped.getVehiclePedIsIn(me, false)
end

-- What the player moves: their vehicle when they drive it, else their ped.
function M.mover(): Entity
	local car = M.vehicle()
	if car and vehicle.getPedInVehicleSeat(car, VehicleSeat.DRIVER, false) == M.me() then
		return car
	end
	return M.me()
end

-- A game text by its label ("VEH_CLASS_0"), in the game's language; `fallback` (the label) when
-- the game has none.
function M.label(name: string, fallback: string?): string
	if name ~= "" and hud.doesTextLabelExist(name) then
		local text = hud.getFilenameForAudioConversation(name)
		if text ~= "" and text ~= "NULL" then
			return text
		end
	end
	return fallback or name
end

-- "WEAPON_ASSAULTRIFLE_MK2" as people read it: "Assaultrifle Mk2" (for names the game has no
-- text for).
function M.pretty(name: string): string
	local words = {}
	for word in string.gmatch(name, "[^_]+") do
		table.insert(words, string.upper(string.sub(word, 1, 1)) .. string.lower(string.sub(word, 2)))
	end
	return table.concat(words, " ")
end

-- The trainer's own notifications, unless the player turned them off.
function M.notify(text: string)
	if prefs.data.notify then
		ui.notify(text)
	end
end

-- Names in alphabetical order, case aside (for table.sort).
function M.byName(a: string, b: string): boolean
	return string.lower(a) < string.lower(b)
end

-- The ground's height at x, y: the world there is loaded first (the player is moved up
-- there while it loads, as teleports must). nil if none was found.
local function findGround(e: Entity, at: vector): number?
	for _, z in { 1000, 800, 600, 400, 300, 200, 150, 100, 75, 50, 25, 10, 0 } do
		local probe = vector.create(at.x, at.y, z)
		entity.setEntityCoordsNoOffset(e, probe, false, false, true)
		streaming.requestCollisionAtCoord(at.x, at.y, z)
		wait(30)
		local found = world.groundZ(probe)
		if found then
			return found
		end
	end
	return nil
end

-- Moves the player (with their vehicle) to `at`; with `ground`, at.z is looked up (a map
-- point has none). Facing `heading` when given.
function M.teleport(at: vector, ground: boolean?, heading: number?)
	local e = M.mover()
	local z = at.z
	if ground then
		local found = findGround(e, at)
		if found then
			z = found + 1
		else
			-- Water or not loaded: from up high, with a parachute.
			z = 1000
			weapon.giveWeaponToPed(M.me(), joaat("GADGET_PARACHUTE"), 1, false, false)
		end
	end
	entity.setEntityCoordsNoOffset(e, vector.create(at.x, at.y, z), false, false, true)
	if heading then
		entity.setEntityHeading(e, heading)
	end
end

-- Where the player is.
function M.position(): vector
	return entity.getEntityCoords(M.me(), true)
end

return M
