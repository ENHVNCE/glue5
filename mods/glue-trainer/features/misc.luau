--!strict
-- The rest: the HUD and the map, the radio and the game's music, cutscenes, the overlay of
-- features/overlay.luau.

local bind = require("../core/bind")
local game = require("../core/game")
local overlay = require("./overlay")
local prefs = require("../core/prefs")
local hudNames = require("../data/hud")

local M = {}

local function screen(menu: Menu)
	menu:separator("HUD and map")
	bind.feature(menu, "misc.noHud", "Hide the HUD", "And the radar: a clean screen", {
		tick = function()
			hud.hideHudAndRadarThisFrame()
		end,
	})
	bind.feature(menu, "misc.noRadar", "Hide the radar", nil, {
		on = function()
			hud.displayRadar(false)
		end,
		off = function()
			hud.displayRadar(true)
		end,
	})
	bind.feature(menu, "misc.revealMap", "Reveal the map", "No fog on the pause menu's map", {
		on = function()
			hud.setMinimapHideFow(true)
		end,
		off = function()
			hud.setMinimapHideFow(false)
		end,
	})
	bind.feature(menu, "misc.bigMap", "Big radar", nil, {
		on = function()
			hud.setBigmapActive(true, false)
		end,
		off = function()
			hud.setBigmapActive(false, false)
		end,
	})
end

local function radio(menu: Menu)
	menu:separator("Radio and music")
	bind.feature(menu, "misc.mobileRadio", "Radio on foot", "The car's radio wherever you are", {
		tick = function()
			audio.setMobileRadioEnabledDuringGameplay(true)
			audio.setMobilePhoneRadioState(true)
		end,
		off = function()
			audio.setMobileRadioEnabledDuringGameplay(false)
			audio.setMobilePhoneRadioState(false)
		end,
	})
	-- The stations the game has unlocked, by their names in the game.
	local stations = menu:choice("Station", { "Radio off" }, 1, "Tune the radio", bind.safe("Station", function(i: number)
		local name = if i == 1 then "OFF" else audio.getRadioStationName(i - 2)
		local car = game.vehicle()
		if car then
			audio.setVehRadioStation(car, name)
		else
			audio.setRadioToStationName(name)
		end
	end))
	menu:onOpen(bind.safe("Misc", function()
		local names = { "Radio off" }
		for i = 0, audio.getNumUnlockedRadioStations() - 1 do
			local id = audio.getRadioStationName(i)
			table.insert(names, game.label(id, id))
		end
		stations.options = names
	end))
	bind.command(menu, "misc.nextSong", "Next song", { action = "Skip" }, function(): string?
		audio.skipRadioForward()
		return nil
	end)
	bind.feature(menu, "misc.noScanner", "No police scanner", "Nor the wanted music", {
		on = function()
			audio.setAudioFlag("PoliceScannerDisabled", true)
			audio.setAudioFlag("WantedMusicDisabled", true)
		end,
		off = function()
			audio.setAudioFlag("PoliceScannerDisabled", false)
			audio.setAudioFlag("WantedMusicDisabled", false)
		end,
	})
end

-- The HUD's colours (hudcolor.dat's), each to any other; kept, and put back on the way out.
local function colours(root: Menu)
	local menu = root:submenu("HUD colours", { note = "The waypoint, the map's blips, the menus' accent...", search = true })
	-- The game's own colour of each changed one, to put back.
	local original: { [number]: Rgba } = {}
	local function set(index: number, c: Rgba)
		if not original[index] then
			local r, g, b, a = hud.getHudColour(index)
			original[index] = { r = r, g = g, b = b, a = a }
		end
		hud.replaceHudColourWithRgba(index, c.r, c.g, c.b, c.a or 255)
	end
	local kept: { [string]: Rgba } = prefs.get("misc.hudColours", {})
	for name, c in kept do
		local i = table.find(hudNames, name)
		if i then
			bind.safe("HUD colours", set)(i - 1, c)
		end
	end
	mod.onStop(function()
		for index, c in original do
			pcall(hud.replaceHudColourWithRgba, index, c.r, c.g, c.b, c.a or 255)
		end
	end)
	bind.action(menu, "The game's own", { note = "Every colour back as it was", action = "Reset" }, function(): string?
		for index, c in original do
			hud.replaceHudColourWithRgba(index, c.r, c.g, c.b, c.a or 255)
		end
		prefs.set("misc.hudColours", {})
		return nil
	end)
	menu:separator()
	local built = false
	menu:onOpen(bind.safe("HUD colours", function()
		if built then
			return
		end
		built = true
		for i, name in hudNames do
			local index = i - 1
			local r, g, b, a = hud.getHudColour(index)
			menu:color(game.pretty(name), { r = r, g = g, b = b, a = a }, { alpha = true }, bind.safe(name, function(c)
				set(index, c)
				local all = prefs.get("misc.hudColours", {})
				all[name] = { r = c.r, g = c.g, b = c.b, a = c.a or 255 }
				prefs.set("misc.hudColours", all)
			end))
		end
	end))
end

-- The game's TV channels, on the whole screen.
local function tv(menu: Menu)
	menu:separator("TV")
	local channel = bind.choice(menu, "misc.tvChannel", "Channel", { "CNT", "Weazel" }, "CNT", "What Watch TV shows")
	bind.feature(menu, "misc.tv", "Watch TV", "The game's TV over the whole screen", {
		on = function()
			graphics.setTvChannel(channel.value - 1)
			graphics.setTvAudioFrontend(true)
			graphics.setTvVolume(0)
		end,
		tick = function()
			graphics.setTvChannel(channel.value - 1)
			graphics.drawTvChannel(0.5, 0.5, 1, 1, 0, 255, 255, 255, 255)
		end,
		off = function()
			graphics.setTvChannel(-1)
			graphics.setTvAudioFrontend(false)
		end,
	})
end

local function story(menu: Menu)
	menu:separator("Story")
	bind.command(menu, "misc.skipCutscene", "Skip the cutscene", { action = "Skip" }, function(): string?
		if not cutscene.isCutsceneActive() then
			return "No cutscene playing"
		end
		cutscene.stopCutsceneImmediately()
		return nil
	end)
	bind.feature(menu, "misc.noPhone", "No phone", "It does not come out", {
		tick = function()
			pad.disableControlAction(0, Control.PHONE, true)
		end,
	})
	bind.feature(menu, "misc.noStuntJumps", "No stunt jumps", "No slow motion over ramps", {
		on = function()
			misc.setStuntJumpsCanTrigger(false)
		end,
		off = function()
			misc.setStuntJumpsCanTrigger(true)
		end,
	})
end

function M.build(root: Menu)
	local menu = root:submenu("Misc", "HUD, its colours, map, radio, TV, cutscenes, overlay")
	overlay.build(menu)
	colours(menu)
	screen(menu)
	radio(menu)
	tv(menu)
	story(menu)
end

return M
