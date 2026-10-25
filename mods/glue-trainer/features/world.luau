--!strict
-- The world: the clock, the weather and the sky, gravity and time's pace, the screen's
-- filters, the streets and the police, the people around, bodyguards.
--
-- In a GTA Online session the clock and the weather are the session's: the trainer changes
-- them for you only (the game's own override), as every player sees their own.

local bind = require("../core/bind")
local game = require("../core/game")
local loop = require("../core/loop")
local catalog = require("../core/catalog")
local data = require("../data/world")

local M = {}

local function online(): boolean
	return network.networkIsSessionStarted()
end

local function setTime(h: number, m: number)
	if online() then
		network.networkOverrideClockTime(h, m, 0)
	else
		clock.setClockTime(h, m, 0)
	end
end

local function time(menu: Menu)
	menu:separator("Time")
	local hour = menu:slider("Hour", clock.getClockHours(), 0, 23, 1, bind.safe("Hour", function(h)
		setTime(h, clock.getClockMinutes())
	end))
	local minute = menu:number("Minute", clock.getClockMinutes(), { min = 0, max = 59, step = 5 }, bind.safe("Minute", function(m)
		setTime(clock.getClockHours(), m)
	end))
	-- The game's clock runs on: shown as it is whenever the menu opens.
	menu:onOpen(bind.safe("World", function()
		hour.value = clock.getClockHours()
		minute.value = clock.getClockMinutes()
	end))
	bind.command(menu, "world.gameTime", "Back to the game's time", { note = "In GTA Online, the session's", action = "Reset" }, function(): string?
		network.networkClearClockTimeOverride()
		return nil
	end)
	bind.feature(menu, "world.freezeClock", "Freeze time", "The clock stops where it is", {
		on = function()
			clock.pauseClock(true)
		end,
		off = function()
			clock.pauseClock(false)
		end,
	})
	bind.feature(menu, "world.realTime", "Real time", "The clock follows this device's", {
		tick = function()
			local now = os.date("*t")
			setTime(now.hour, now.min)
		end,
	})
	bind.slider(menu, "world.timeScale", "Game speed", 1, 0.1, 1, { step = 0.1, unit = "%", note = "Slow motion below 100 %" }, function(v)
		misc.setTimeScale(v)
	end)
end

local function weather(menu: Menu)
	menu:separator("Weather and sea")
	local names = {}
	for _, w in data.weathers do
		table.insert(names, w[2])
	end
	menu:choice("Weather", names, 1, "Changes at once", bind.safe("Weather", function(i: number)
		local name = data.weathers[i][1]
		if online() then
			misc.setOverrideWeather(name)
		else
			misc.setWeatherTypeNowPersist(name)
		end
	end))
	bind.command(menu, "world.gameWeather", "Back to the game's weather", { action = "Reset" }, function(): string?
		misc.clearOverrideWeather()
		misc.clearWeatherTypePersist()
		return nil
	end)
	bind.slider(menu, "world.wind", "Wind", 0, 0, 12, { step = 1, unit = "m/s" }, function(v)
		misc.setWindSpeed(v)
	end)
	bind.slider(menu, "world.waves", "Waves", 1, 0, 5, { step = 0.5, unit = "×", note = "How high the sea's waves are" }, function(v)
		water.setDeepOceanScaler(v)
	end)
	bind.feature(menu, "world.blackout", "Blackout", "Every light of the city off", {
		on = function()
			graphics.setArtificialLightsState(true)
		end,
		off = function()
			graphics.setArtificialLightsState(false)
		end,
	})
	bind.choice(menu, "world.gravity", "Gravity", data.gravity, data.gravity[1], nil, function(i)
		misc.setGravityLevel(i - 1)
	end)
end

local function filters(root: Menu)
	local menu = root:submenu("Screen filters", { note = "The game's own looks", search = true })
	local strength = bind.slider(menu, "world.filterStrength", "Strength", 1, 0, 1, { step = 0.05, unit = "%" }, function(v)
		graphics.setTimecycleModifierStrength(v)
	end)
	bind.command(menu, "world.noFilter", "None", { action = "Clear" }, function(): string?
		graphics.clearTimecycleModifier()
		return nil
	end)
	menu:separator()
	for _, f in data.filters do
		bind.action(menu, f[1], { action = "Use" }, function(): string?
			graphics.setTimecycleModifier(f[2])
			graphics.setTimecycleModifierStrength(strength.value)
			return nil
		end)
	end
end

local function streets(menu: Menu)
	menu:separator("Streets and police")
	local traffic, people = 1, 1
	local function busy()
		vehicle.setVehicleDensityMultiplierThisFrame(traffic)
		vehicle.setRandomVehicleDensityMultiplierThisFrame(traffic)
		vehicle.setParkedVehicleDensityMultiplierThisFrame(traffic)
		ped.setPedDensityMultiplierThisFrame(people)
		ped.setScenarioPedDensityMultiplierThisFrame(people, people)
	end
	-- The game asks again every frame: the loop runs only while one is not 100 %.
	local function apply()
		if traffic < 1 or people < 1 then
			loop.add("world.density", busy)
		else
			loop.remove("world.density")
		end
	end
	bind.slider(menu, "world.traffic", "Traffic", 1, 0, 1, { step = 0.1, unit = "%" }, function(v)
		traffic = v
		apply()
	end)
	bind.slider(menu, "world.people", "People", 1, 0, 1, { step = 0.1, unit = "%" }, function(v)
		people = v
		apply()
	end)
	bind.feature(menu, "world.noTrains", "No trains or boats", "The city's own, not yours", {
		on = function()
			vehicle.setRandomTrains(false)
			vehicle.setRandomBoats(false)
		end,
		off = function()
			vehicle.setRandomTrains(true)
			vehicle.setRandomBoats(true)
		end,
	})
	bind.feature(menu, "world.noDispatch", "No police dispatch", "No patrols, helicopters, roadblocks or SWAT sent after you", {
		on = function()
			for service = 1, data.dispatchServices do
				misc.enableDispatchService(service, false)
			end
			ped.setCreateRandomCops(false)
		end,
		off = function()
			for service = 1, data.dispatchServices do
				misc.enableDispatchService(service, true)
			end
			ped.setCreateRandomCops(true)
		end,
	})
	-- The scripts start again when the player comes back: ended again every few seconds.
	local since = 0
	bind.feature(menu, "world.restricted", "Go anywhere", "The army base, the prison, the airport let you in", {
		tick = function()
			since += misc.getFrameTime()
			if since < 3 then
				return
			end
			since = 0
			for _, name in data.restrictedAreas do
				misc.terminateAllScriptsWithThisName(name)
			end
		end,
	})
	bind.feature(menu, "world.riot", "Riot", "Everyone fights everyone", {
		on = function()
			misc.setRiotModeEnabled(true)
		end,
		off = function()
			misc.setRiotModeEnabled(false)
		end,
	})
end

local function around(menu: Menu)
	menu:separator("Around you")
	bind.command(menu, "world.clear", "Clear the area", { note = "Vehicles, people, objects and fires within 100 m", action = "Clear" }, function(): string?
		misc.clearArea(game.position(), 100, true, false, false, false)
		return nil
	end)
	bind.command(menu, "world.knockOut", "Knock out everyone", { note = "People within 100 m, not players", action = "Run" }, function(): string?
		local count = 0
		for _, p in world.nearby("peds", game.me(), 100) do
			if not ped.isPedAPlayer(p) and not ped.isPedDeadOrDying(p, true) then
				entity.setEntityHealth(p, 0, nil)
				count += 1
			end
		end
		return `{count} people`
	end)
	bind.command(menu, "world.disarm", "Disarm everyone", { note = "People within 100 m, not players", action = "Run" }, function(): string?
		for _, p in world.nearby("peds", game.me(), 100) do
			if not ped.isPedAPlayer(p) then
				weapon.removeAllPedWeapons(p, true)
			end
		end
		return nil
	end)
	bind.command(menu, "world.explode", "Blow up the vehicles", { note = "Within 100 m, not yours", action = "Boom" }, function(): string?
		local mine = game.vehicle()
		local count = 0
		for _, car in world.nearby("vehicles", game.me(), 100) do
			if car ~= mine then
				vehicle.explodeVehicle(car, true, false)
				count += 1
			end
		end
		return `{count} vehicles`
	end)
	bind.command(menu, "world.launch", "Launch the vehicles", { note = "Within 100 m, up in the air", action = "Launch" }, function(): string?
		local mine = game.vehicle()
		for _, car in world.nearby("vehicles", game.me(), 100) do
			if car ~= mine then
				entity.applyForceToEntityCenterOfMass(car, 1, 0, 0, 30, true, false, true, false)
			end
		end
		return nil
	end)
end

-- People who follow and protect you: any ped of the game, with any weapon.
local function bodyguards(root: Menu)
	local menu = root:submenu("Bodyguards", "Any ped of the game, armed, by your side")
	local guards: { Ped } = {}
	local built = false
	local function dismiss()
		for _, g in guards do
			if entity.doesEntityExist(g) then
				ped.removePedFromGroup(g)
				entity.setEntityAsNoLongerNeeded(g)
			end
		end
		guards = {}
	end
	mod.onStop(dismiss)
	menu:onOpen(bind.safe("Bodyguards", function()
		if built then
			return
		end
		local peds = catalog.peds()
		local weapons = { "Unarmed" }
		for _, w in catalog.weapons() do
			table.insert(weapons, w.model)
		end
		if #peds == 0 then
			return
		end
		built = true
		local modelItem = menu:choice("Model", peds, if table.find(peds, "s_m_y_swat_01") then "s_m_y_swat_01" else 1)
		local weaponItem = menu:choice("Weapon", weapons, if table.find(weapons, "WEAPON_CARBINERIFLE") then "WEAPON_CARBINERIFLE" else 1)
		bind.action(menu, "Call one", { action = "Call" }, function(): string?
			local hash = joaat(modelItem.option)
			if not streaming.load(hash) then
				return "That model did not load"
			end
			local me = game.me()
			local at = entity.getOffsetFromEntityInWorldCoords(me, vector.create(math.random(-3, 3), -2, 0))
			local g = ped.createPed(26, hash, at, entity.getEntityHeading(me), false, false)
			streaming.setModelAsNoLongerNeeded(hash)
			if not g then
				return "The game made none"
			end
			ped.setPedAsGroupMember(g, player.getPlayerGroup(game.player()))
			ped.setPedNeverLeavesGroup(g, true)
			ped.setPedCombatAbility(g, 2)
			entity.setEntityInvincible(g, false)
			if weaponItem.value > 1 then
				weapon.giveWeaponToPed(g, joaat(weaponItem.option), 9999, false, true)
			end
			table.insert(guards, g)
			return `{#guards} by your side`
		end)
		bind.action(menu, "Dismiss them", { action = "Dismiss" }, function(): string?
			dismiss()
			return nil
		end)
	end))
end

function M.build(root: Menu)
	local menu = root:submenu("World", "Time, weather, filters, traffic, police, the area around, bodyguards")
	time(menu)
	weather(menu)
	filters(menu)
	bodyguards(menu)
	streets(menu)
	around(menu)
end

return M
