--!strict
-- The vehicle the player is in: fix it, keep it whole, doors and windows, make it faster or
-- lighter, let it drive itself, and its customs (features/customs.luau).

local bind = require("../core/bind")
local game = require("../core/game")
local loop = require("../core/loop")
local customs = require("./customs")
local data = require("../data/vehicles")

local M = {}

-- `f` with the player's vehicle, or the message that there is none.
local function withVehicle(f: (car: Vehicle) -> string?): () -> string?
	return function()
		local car = game.vehicle()
		if not car then
			return "You are not in a vehicle"
		end
		return f(car)
	end
end

-- A feature that does `apply` to the vehicle the player is in, every frame, and `release` to
-- it once they leave it (or the feature goes off): it is as the game made it again.
local function perVehicle(apply: (car: Vehicle) -> (), release: ((car: Vehicle) -> ())?): bind.Feature
	local last: Vehicle? = nil
	local function leave()
		if last and release and entity.doesEntityExist(last) then
			release(last)
		end
		last = nil
	end
	return {
		tick = function()
			local car = game.vehicle()
			if car ~= last then
				leave()
				last = car
			end
			if car then
				apply(car)
			end
		end,
		off = leave,
	}
end

local function fix(car: Vehicle)
	vehicle.setVehicleFixed(car)
	vehicle.setVehicleDeformationFixed(car)
	vehicle.setVehicleDirtLevel(car, 0)
	vehicle.setVehicleEngineHealth(car, 1000)
end

local function actions(menu: Menu)
	bind.command(menu, "vehicle.repair", "Repair", { note = "Body, engine, tyres and dirt", action = "Repair" }, withVehicle(function(car: Vehicle): string?
		fix(car)
		return "Repaired"
	end))
	bind.command(menu, "vehicle.flip", "Flip upright", { action = "Flip" }, withVehicle(function(car: Vehicle): string?
		local rot = entity.getEntityRotation(car, 2)
		entity.setEntityRotation(car, vector.create(0, 0, rot.z), 2, true)
		vehicle.setVehicleOnGroundProperly(car, 5)
		return nil
	end))
	bind.command(menu, "vehicle.engine", "Engine on or off", { action = "Switch" }, withVehicle(function(car: Vehicle): string?
		vehicle.setVehicleEngineOn(car, not vehicle.getIsVehicleEngineRunning(car), true, true)
		return nil
	end))
	bind.command(menu, "vehicle.last", "Back in your last vehicle", { note = "Wherever it is", action = "Get in" }, function(): string?
		local car = player.getPlayersLastVehicle()
		if not car or not entity.doesEntityExist(car) then
			return "No last vehicle"
		end
		ped.setPedIntoVehicle(game.me(), car, VehicleSeat.DRIVER)
		return nil
	end)
	bind.command(menu, "vehicle.delete", "Delete", { note = "The vehicle goes; you stay", action = "Delete" }, withVehicle(function(car: Vehicle): string?
		local me = game.me()
		entity.setEntityCoordsNoOffset(me, entity.getEntityCoords(car, true) + vector.create(0, 0, 1), false, false, true)
		entity.setEntityAsMissionEntity(car, true, true)
		vehicle.deleteVehicle(car)
		return nil
	end))
	local seat = menu:choice("Seat", { "Driver" }, 1, "Move to another seat", bind.safe("Seat", function(i: number)
		local car = game.vehicle()
		if not car then
			return
		end
		-- Seats count from -1, the driver's.
		local index = i - 2
		if vehicle.isVehicleSeatFree(car, index, false) then
			ped.setPedIntoVehicle(game.me(), car, index)
		else
			game.notify("That seat is taken")
		end
	end))
	-- The seats of the vehicle the player is in when the menu opens.
	menu:onOpen(bind.safe("Vehicle", function()
		local car = game.vehicle()
		local options = { "Driver" }
		if car then
			for n = 1, vehicle.getVehicleMaxNumberOfPassengers(car) do
				table.insert(options, `Passenger {n}`)
			end
		end
		seat.options = options
		seat.enabled = car ~= nil
	end))
end

local function doors(root: Menu)
	local menu = root:submenu("Doors and windows")
	bind.command(menu, "vehicle.doorsOpen", "Open every door", { action = "Open" }, withVehicle(function(car: Vehicle): string?
		for door = 0, 5 do
			vehicle.setVehicleDoorOpen(car, door, false, false)
		end
		return nil
	end))
	bind.command(menu, "vehicle.doorsShut", "Shut every door", { action = "Shut" }, withVehicle(function(car: Vehicle): string?
		vehicle.setVehicleDoorsShut(car, false)
		return nil
	end))
	menu:separator("One door")
	-- The doors, from 0 (front left) to 5 (trunk).
	for door = 0, 5 do
		local name = data.doors[door]
		local open = false
		bind.action(menu, name, { action = "Open / shut" }, withVehicle(function(car: Vehicle): string?
			open = not open
			if open then
				vehicle.setVehicleDoorOpen(car, door, false, false)
			else
				vehicle.setVehicleDoorShut(car, door, false)
			end
			return nil
		end))
	end
	menu:separator("Windows and locks")
	bind.action(menu, "Windows down", { action = "Down" }, withVehicle(function(car: Vehicle): string?
		vehicle.rollDownWindows(car)
		return nil
	end))
	bind.action(menu, "Windows up", { action = "Up" }, withVehicle(function(car: Vehicle): string?
		for window = 0, 3 do
			vehicle.rollUpWindow(car, window)
		end
		return nil
	end))
	bind.feature(menu, "vehicle.locked", "Locked", "Nobody gets in", perVehicle(function(car)
		vehicle.setVehicleDoorsLocked(car, 2)
	end, function(car)
		vehicle.setVehicleDoorsLocked(car, 1)
	end))
end

local function protection(menu: Menu)
	menu:separator("Protection")
	bind.feature(menu, "vehicle.god", "Indestructible", "The vehicle you are in takes no damage", perVehicle(function(car)
		entity.setEntityInvincible(car, true)
		vehicle.setVehicleCanBeVisiblyDamaged(car, false)
		vehicle.setVehicleTyresCanBurst(car, false)
	end, function(car)
		entity.setEntityInvincible(car, false)
		vehicle.setVehicleCanBeVisiblyDamaged(car, true)
		vehicle.setVehicleTyresCanBurst(car, true)
	end))
	bind.feature(menu, "vehicle.autoRepair", "Repair itself", "Fixed as soon as it is damaged", perVehicle(function(car)
		if vehicle.getVehicleBodyHealth(car) < 1000 or vehicle.getVehicleEngineHealth(car) < 1000 then
			fix(car)
		end
	end))
	bind.feature(menu, "vehicle.clean", "Always clean", nil, perVehicle(function(car)
		vehicle.setVehicleDirtLevel(car, 0)
	end))
	bind.feature(menu, "vehicle.autoFlip", "Lands on its wheels", "Turned over: back upright", perVehicle(function(car)
		if math.abs(entity.getEntityRoll(car)) > 90 and not entity.isEntityInAir(car) then
			vehicle.setVehicleOnGroundProperly(car, 5)
		end
	end))
	bind.feature(menu, "vehicle.seatbelt", "Seatbelt", "Never thrown through the windscreen, never knocked off a bike", {
		tick = function()
			-- 32: PED_FLAG_CAN_FLY_THRU_WINDSCREEN.
			ped.setPedConfigFlag(game.me(), 32, false)
			ped.setPedCanBeKnockedOffVehicle(game.me(), 1)
		end,
		off = function()
			ped.setPedConfigFlag(game.me(), 32, true)
			ped.setPedCanBeKnockedOffVehicle(game.me(), 0)
		end,
	})
	bind.feature(menu, "vehicle.noDragOut", "Nobody pulls you out", nil, {
		tick = function()
			ped.setPedCanBeDraggedOut(game.me(), false)
		end,
		off = function()
			ped.setPedCanBeDraggedOut(game.me(), true)
		end,
	})
end

local function driving(menu: Menu)
	menu:separator("Driving")
	bind.feature(menu, "vehicle.hornBoost", "Horn boost", "The horn pushes you forward", perVehicle(function(car)
		if pad.isControlPressed(0, Control.VEH_HORN) then
			vehicle.setVehicleForwardSpeed(car, entity.getEntitySpeed(car) + 3)
		end
	end))
	bind.feature(menu, "vehicle.instantBrake", "Instant brake", "The handbrake stops it dead", perVehicle(function(car)
		if pad.isControlPressed(0, Control.VEH_HANDBRAKE) then
			entity.setEntityVelocity(car, vector.create(0, 0, 0))
		end
	end))
	bind.command(menu, "vehicle.jump", "Jump", { note = "Up in the air (give it a key)", action = "Jump" }, withVehicle(function(car: Vehicle): string?
		entity.applyForceToEntityCenterOfMass(car, 1, 0, 0, 12, true, true, true, true)
		return nil
	end))
	-- Extra power and top speed, given every frame to the vehicle the player is in.
	local power, top = 0, 0
	local function boost()
		if power > 0 or top > 0 then
			loop.add("vehicle.power", function()
				local car = game.vehicle()
				if car then
					vehicle.setVehicleCheatPowerIncrease(car, 1 + power)
					vehicle.modifyVehicleTopSpeed(car, top * 100)
				end
			end)
		else
			loop.remove("vehicle.power")
			local car = game.vehicle()
			if car then
				vehicle.setVehicleCheatPowerIncrease(car, 1)
				vehicle.modifyVehicleTopSpeed(car, 0)
			end
		end
	end
	bind.slider(menu, "vehicle.power", "Engine power", 0, 0, 5, { step = 0.1, unit = "%", note = "Extra power over the engine's" }, function(v)
		power = v
		boost()
	end)
	bind.slider(menu, "vehicle.topSpeed", "Top speed", 0, 0, 5, { step = 0.1, unit = "%", note = "Faster than it was built for" }, function(v)
		top = v
		boost()
	end)
	bind.feature(menu, "vehicle.drift", "Drift tyres", "Little grip, long slides", perVehicle(function(car)
		vehicle.setVehicleReduceGrip(car, true)
	end, function(car)
		vehicle.setVehicleReduceGrip(car, false)
	end))
	bind.feature(menu, "vehicle.noGravity", "No gravity", "Drive off a cliff and keep going", perVehicle(function(car)
		vehicle.setVehicleGravity(car, false)
	end, function(car)
		vehicle.setVehicleGravity(car, true)
	end))
	bind.feature(menu, "vehicle.invisible", "Invisible", "Others do not see it; you still do", perVehicle(function(car)
		entity.setEntityVisible(car, false, false)
		network.setEntityLocallyVisible(car)
	end, function(car)
		entity.setEntityVisible(car, true, false)
	end))
	-- Through the colour wheel, one turn every 6 seconds.
	bind.feature(menu, "vehicle.rainbow", "Rainbow paint", nil, perVehicle(function(car)
		local h = (os.clock() / 6) % 1
		local function channel(n: number): number
			local k = (n + h * 6) % 6
			return math.floor(255 * (1 - math.max(0, math.min(k, 4 - k, 1))) + 0.5)
		end
		local r, g, b = channel(5), channel(3), channel(1)
		vehicle.setVehicleCustomPrimaryColour(car, r, g, b)
		vehicle.setVehicleCustomSecondaryColour(car, r, g, b)
	end))
end

-- The vehicle drives itself: to the waypoint, or around.
local function autopilot(root: Menu)
	local menu = root:submenu("Autopilot", "The vehicle drives you")
	local styles = {}
	for _, s in data.drivingStyles do
		table.insert(styles, s.name)
	end
	local style = bind.choice(menu, "vehicle.drivingStyle", "Driving", styles, styles[1])
	local speed = bind.slider(menu, "vehicle.autopilotSpeed", "Speed", 25, 5, 60, { step = 5, unit = "m/s" })
	local function drive(to: vector?): string?
		local car = game.vehicle()
		if not car then
			return "You are not in a vehicle"
		end
		local me = game.me()
		if vehicle.getPedInVehicleSeat(car, VehicleSeat.DRIVER, false) ~= me then
			return "Take the wheel first"
		end
		local mode = data.drivingStyles[style.value].flags
		if to then
			task.taskVehicleDriveToCoordLongrange(me, car, to, speed.value, mode, 10)
		else
			task.taskVehicleDriveWander(me, car, speed.value, mode)
		end
		return nil
	end
	bind.command(menu, "vehicle.toWaypoint", "Drive to the waypoint", { action = "Go" }, function(): string?
		local at = world.waypoint()
		if not at then
			return "No waypoint on the map"
		end
		return drive(at)
	end)
	bind.command(menu, "vehicle.wander", "Drive around", { action = "Go" }, function(): string?
		return drive(nil)
	end)
	bind.command(menu, "vehicle.stopDriving", "Stop", { note = "The wheel is yours again", action = "Stop" }, function(): string?
		task.clearPedTasks(game.me())
		return nil
	end)
end

function M.build(root: Menu)
	local menu = root:submenu("Vehicle", "The one you are in: repair, protect, tune, doors, autopilot")
	actions(menu)
	customs.build(menu)
	doors(menu)
	autopilot(menu)
	protection(menu)
	driving(menu)
end

return M
