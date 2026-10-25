--!strict
-- Going places: the waypoint (now or whenever one is set), the mission's objective, the
-- places of data/places.luau, the player's own saved spots, coordinates, the nearest vehicle.

local bind = require("../core/bind")
local game = require("../core/game")
local places = require("../data/places")

type Spot = { name: string, x: number, y: number, z: number, heading: number }

local M = {}

local function toWaypoint(): string?
	local at = world.waypoint()
	if not at then
		return "No waypoint on the map"
	end
	game.teleport(at, true)
	return nil
end

-- Where the player is, named after the street and the area.
local function here(name: string?): Spot
	local pos = game.position()
	if not name or name == "" then
		local street = world.streetName(pos)
		local area = world.zoneName(pos)
		name = if street ~= "" then `{street}, {area}` else area
	end
	return { name = name :: string, x = pos.x, y = pos.y, z = pos.z, heading = entity.getEntityHeading(game.mover()) }
end

-- The player's spots, kept in the mod's storage.
local function saved(): { Spot }
	return storage.get("spots") or {}
end

local function spots(root: Menu)
	local menu = root:submenu("Saved spots", "Yours: save where you are, come back later")
	local function build()
		menu:clear()
		local list = saved()
		bind.action(menu, "Save here", { note = "Named after the street", action = "Save" }, function(): string?
			local s = saved()
			table.insert(s, here())
			storage.set("spots", s)
			build()
			return `Saved: {s[#s].name}`
		end)
		menu:text("Save here as", "", { placeholder = "A name, then Enter", max = 40 }, bind.safe("Save here as", function(name)
			if name == "" then
				return
			end
			local s = saved()
			table.insert(s, here(name))
			storage.set("spots", s)
			build()
			game.notify(`Saved: {name}`)
		end))
		if #list == 0 then
			menu:label("Nothing saved yet.")
			return
		end
		menu:separator("Go")
		for _, spot in list do
			bind.action(menu, spot.name, { note = world.zoneName(vector.create(spot.x, spot.y, spot.z)), action = "Go" }, function(): string?
				game.teleport(vector.create(spot.x, spot.y, spot.z), false, spot.heading)
				return nil
			end)
		end
		local forget = menu:submenu("Delete a spot")
		for i, spot in list do
			bind.action(forget, spot.name, { action = "Delete" }, function(): string?
				local s = saved()
				table.remove(s, i)
				storage.set("spots", s)
				build()
				return `Deleted: {spot.name}`
			end)
		end
	end
	build()
end

-- Mission objectives are standard blips (sprite 1) in the game's yellows.
local OBJECTIVE_SPRITE = 1
local OBJECTIVE_COLOURS = { 5, 46, 60, 66 }

-- Where the mission wants the player: a yellow blip, else any standard one.
local function objective(): vector?
	local any: vector? = nil
	local blip = hud.getFirstBlipInfoId(OBJECTIVE_SPRITE)
	while blip and hud.doesBlipExist(blip) do
		local at = hud.getBlipInfoIdCoord(blip)
		if table.find(OBJECTIVE_COLOURS, hud.getBlipColour(blip)) then
			return at
		end
		any = any or at
		blip = hud.getNextBlipInfoId(OBJECTIVE_SPRITE)
	end
	return any
end

local function coordinates(root: Menu)
	local menu = root:submenu("Coordinates", "Anywhere, by its numbers")
	local x = menu:number("X", 0, { step = 10 })
	local y = menu:number("Y", 0, { step = 10 })
	local z = menu:number("Z", 0, { step = 5, note = "0: on the ground" })
	bind.action(menu, "Go there", { action = "Go" }, function(): string?
		game.teleport(vector.create(x.value, y.value, z.value), z.value == 0)
		return nil
	end)
	bind.action(menu, "Where you are", { note = "These numbers, from where you stand", action = "Fill" }, function(): string?
		local pos = game.position()
		x.value = math.floor(pos.x * 10 + 0.5) / 10
		y.value = math.floor(pos.y * 10 + 0.5) / 10
		z.value = math.floor(pos.z * 10 + 0.5) / 10
		return nil
	end)
end

function M.build(root: Menu)
	local menu = root:submenu("Teleport", "Waypoint, objective, places, your saved spots")
	bind.command(menu, "teleport.waypoint", "To the waypoint", { note = "The map's marker", action = "Go" }, toWaypoint)
	bind.command(menu, "teleport.objective", "To the objective", { note = "Where the mission wants you", action = "Go" }, function(): string?
		local at = objective()
		if not at then
			return "No objective on the map"
		end
		game.teleport(at, true)
		return nil
	end)
	-- A new waypoint takes you there (a teleport waits for the ground: a task of its own).
	local seen: vector? = nil
	bind.feature(menu, "teleport.auto", "Go to every new waypoint", "Mark the map, you are there", {
		on = function()
			seen = world.waypoint()
		end,
		tick = function()
			local at = world.waypoint()
			if at and (not seen or vector.magnitude(at - (seen :: vector)) > 1) then
				task.spawn(game.teleport, at, true)
			end
			seen = at
		end,
	})
	bind.command(menu, "teleport.forward", "Forward", { note = "10 m ahead, through walls", action = "Go" }, function(): string?
		local e = game.mover()
		local ahead = entity.getEntityCoords(e, true) + entity.getEntityForwardVector(e) * 10
		game.teleport(ahead, false)
		return nil
	end)
	bind.command(menu, "teleport.nearestVehicle", "Into the nearest vehicle", { note = "A free seat within 50 m", action = "Get in" }, function(): string?
		local me = game.me()
		for _, car in world.nearby("vehicles", me, 50) do
			for seat = -1, vehicle.getVehicleMaxNumberOfPassengers(car) - 1 do
				if vehicle.isVehicleSeatFree(car, seat, false) then
					ped.setPedIntoVehicle(me, car, seat)
					return nil
				end
			end
		end
		return "No free seat nearby"
	end)
	spots(menu)
	coordinates(menu)
	menu:separator("Places")
	for _, kind in places do
		local sub = menu:submenu(kind.name, `{#kind.places} places`)
		for _, place in kind.places do
			bind.action(sub, place.name, { action = "Go" }, function(): string?
				game.teleport(vector.create(place.x, place.y, place.z), false, place.heading)
				return nil
			end)
		end
	end
end

return M
