--!strict
-- The other players of a GTA Online session, friends: how they are, go to them or into their
-- vehicle, watch them, mark them on the map, wear their outfit, name tags. Listed as they are
-- each time the menu opens.
--
-- Their ped and the vehicle they drive are their game's, and what a mod makes is this player's
-- own (a mod's script is no network script of the game's): no gifts.

local bind = require("../core/bind")
local game = require("../core/game")

local M = {}

-- The game's player slots.
local SLOTS = 32

local watching: Player? = nil

local function stopWatching()
	if watching then
		network.networkSetInSpectatorMode(false, game.me())
		watching = nil
	end
end

mod.onStop(stopWatching)

local function about(who: Player): string
	local p = player.getPlayerPed(who)
	local parts = { `{entity.getEntityHealth(p)} health`, `{ped.getPedArmour(p)} armour` }
	local car = ped.getVehiclePedIsIn(p, false)
	if car then
		table.insert(parts, game.label(vehicle.getDisplayNameFromVehicleModel(entity.getEntityModel(car))))
	end
	table.insert(parts, `{math.floor(geo.distance(game.position(), entity.getEntityCoords(p, true)))} m away`)
	return table.concat(parts, " · ")
end

local function playerMenu(menu: Menu, who: Player, name: string)
	local sub = menu:submenu(name, about(who))
	local them = function(): Ped
		return player.getPlayerPed(who)
	end
	sub:separator("Go")
	bind.action(sub, "Go to them", { action = "Go" }, function(): string?
		game.teleport(entity.getEntityCoords(them(), true) + vector.create(0, 0, 1), false)
		return nil
	end)
	bind.action(sub, "Into their vehicle", { note = "A free seat", action = "Get in" }, function(): string?
		local car = ped.getVehiclePedIsIn(them(), false)
		if not car then
			return `{name} is on foot`
		end
		for seat = 0, vehicle.getVehicleMaxNumberOfPassengers(car) - 1 do
			if vehicle.isVehicleSeatFree(car, seat, false) then
				ped.setPedIntoVehicle(game.me(), car, seat)
				return nil
			end
		end
		return "No free seat"
	end)
	bind.action(sub, "Mark them on the map", { note = "The waypoint where they are now", action = "Mark" }, function(): string?
		world.setWaypoint(entity.getEntityCoords(them(), true))
		return nil
	end)
	sub:toggle("Watch them", watching == who, "Your view follows them", bind.safe("Watch them", function(on)
		stopWatching()
		if on then
			network.networkSetInSpectatorMode(true, them())
			watching = who
		end
	end))
	sub:separator("Them")
	bind.action(sub, "Wear their outfit", { note = "When you are the same model", action = "Copy" }, function(): string?
		local me, p = game.me(), them()
		if entity.getEntityModel(me) ~= entity.getEntityModel(p) then
			return "You are another model"
		end
		for slot = 0, 11 do
			ped.setPedComponentVariation(me, slot, ped.getPedDrawableVariation(p, slot), ped.getPedTextureVariation(p, slot), ped.getPedPaletteVariation(p, slot))
		end
		for _, slot in { 0, 1, 2, 6, 7 } do
			local prop = ped.getPedPropIndex(p, slot)
			if prop < 0 then
				ped.clearPedProp(me, slot)
			else
				ped.setPedPropIndex(me, slot, prop, ped.getPedPropTextureIndex(p, slot), true)
			end
		end
		return nil
	end)
end

-- Name tags over every other player's head, kept up as they come and go.
local function tags(menu: Menu)
	local shown: { [number]: number } = {}
	local function clear()
		for _, tag in shown do
			hud.removeMpGamerTag(tag)
		end
		shown = {}
	end
	bind.feature(menu, "players.tags", "Name tags", "Over every player's head", {
		tick = function()
			local me = game.player()
			for slot = 0, SLOTS - 1 do
				local who = player.intToPlayerindex(slot)
				local active = who ~= me and network.networkIsPlayerActive(who)
				if active and not shown[slot] then
					local tag = hud.createFakeMpGamerTag(player.getPlayerPed(who), player.getPlayerName(who), false, false, "", 0)
					hud.setMpGamerTagVisibility(tag, 0, true, 0)
					shown[slot] = tag
				elseif not active and shown[slot] then
					hud.removeMpGamerTag(shown[slot])
					shown[slot] = nil
				end
			end
		end,
		off = clear,
	})
end

function M.build(root: Menu)
	local menu = root:submenu("Players", "The other players of a GTA Online session")
	local list = menu:submenu("Who is here", "Go to, watch, give: one by one")
	tags(menu)
	bind.feature(menu, "players.friendlyFire", "Friendly fire", "Hurt your friends and crew", {
		on = function()
			network.networkSetFriendlyFireOption(true)
			ped.setCanAttackFriendly(game.me(), true, false)
		end,
		off = function()
			network.networkSetFriendlyFireOption(false)
			ped.setCanAttackFriendly(game.me(), false, false)
		end,
	})
	list:onOpen(bind.safe("Players", function()
		list:clear()
		if not network.networkIsSessionStarted() then
			list:label("Not in a GTA Online session.")
			return
		end
		local me = game.player()
		local count = 0
		for slot = 0, SLOTS - 1 do
			local who = player.intToPlayerindex(slot)
			if who ~= me and network.networkIsPlayerActive(who) then
				playerMenu(list, who, player.getPlayerName(who))
				count += 1
			end
		end
		if count == 0 then
			list:label("Nobody else is here.")
		end
		if watching then
			bind.action(list, "Stop watching", { action = "Stop" }, function(): string?
				stopWatching()
				return nil
			end)
		end
	end))
end

return M
