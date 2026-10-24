--!strict
-- The rest: the HUD and the map, the radio and the game's music, cutscenes, the overlay of
-- features/overlay.luau.

local bind = require("../core/bind")
local game = require("../core/game")
local overlay = require("./overlay")

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

local function story(menu: Menu)
	menu:separator("Story")
	bind.command(menu, "misc.skipCutscene", "Skip the cutscene", { action = "Skip" }, function(): string?
		if not cutscene.isCutsceneActive() then
			return "No cutscene playing"
		end
		cutscene.stopCutsceneImmediately()
		return nil
	end)
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
	local menu = root:submenu("Misc", "HUD, map, radio, cutscenes, overlay")
	overlay.build(menu)
	screen(menu)
	radio(menu)
	story(menu)
end

return M
