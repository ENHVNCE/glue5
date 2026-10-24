--!strict
-- How you look and move: any ped model of the game, every piece of clothing it has, saved
-- outfits, animations, scenarios, walks and moods.

local bind = require("../core/bind")
local game = require("../core/game")
local catalog = require("../core/catalog")
local lists = require("../core/lists")
local data = require("../data/looks")

local M = {}

type Outfit = { name: string, model: number, components: { { number } }, props: { { number } } }

local function model(root: Menu)
	local start = entity.getEntityModel(game.me())
	local function become(hash: number): string?
		if not streaming.load(hash) then
			return "That model did not load"
		end
		player.setPlayerModel(game.player(), hash)
		streaming.setModelAsNoLongerNeeded(hash)
		ped.setPedDefaultComponentVariation(game.me())
		return nil
	end
	local models = root:submenu("Model", "Any ped of the game")
	bind.command(models, "looks.ownModel", "Back to your own", { note = "The model you started with", action = "Change" }, function(): string?
		return become(start)
	end)
	local function fill(menu: Menu, names: { string }): boolean
		for _, name in names do
			bind.action(menu, name, { action = "Become" }, function(): string?
				return become(joaat(name))
			end)
		end
		return #names > 0
	end
	lists.lazy(models, "Search all", "Every model, by name", function(menu)
		return fill(menu, catalog.peds())
	end)
	models:separator("By kind")
	-- The kinds come from the game's models: added under the heading once they are read.
	local listed = false
	models:onOpen(bind.safe("Model", function()
		if listed then
			return
		end
		local groups = catalog.pedGroups()
		for _, group in groups do
			lists.lazy(models, group.name, `{#group.models} models`, function(menu)
				return fill(menu, group.models)
			end)
		end
		listed = #groups > 0
	end))
end

-- Every clothing slot and prop of the ped you are now, with as many choices as it has.
local function wardrobe(root: Menu)
	local menu = root:submenu("Wardrobe", "Each piece of clothing your model has")
	local function build()
		menu:clear()
		local me = game.me()
		bind.action(menu, "Random outfit", { action = "Dress" }, function(): string?
			ped.setPedRandomComponentVariation(game.me(), 0)
			build()
			return nil
		end)
		bind.action(menu, "Default outfit", { action = "Dress" }, function(): string?
			ped.setPedDefaultComponentVariation(game.me())
			ped.clearAllPedProps(game.me())
			build()
			return nil
		end)
		menu:separator("Clothes")
		for slot = 0, 11 do
			local count = ped.getNumberOfPedDrawableVariations(me, slot)
			if count > 1 then
				local name = data.components[slot]
				local drawable = ped.getPedDrawableVariation(me, slot)
				local textures = math.max(ped.getNumberOfPedTextureVariations(me, slot, drawable), 1)
				local texture: Number
				menu:number(name, drawable, { min = 0, max = count - 1, note = `{count} kinds` }, bind.safe(name, function(d)
					ped.setPedComponentVariation(game.me(), slot, d, 0, 0)
					texture.max = math.max(ped.getNumberOfPedTextureVariations(game.me(), slot, d), 1) - 1
					texture.value = 0
				end))
				texture = menu:number(`{name}: colour`, ped.getPedTextureVariation(me, slot), { min = 0, max = textures - 1 }, bind.safe(name, function(t)
					ped.setPedComponentVariation(game.me(), slot, ped.getPedDrawableVariation(game.me(), slot), t, 0)
				end))
			end
		end
		menu:separator("Props")
		for slot, name in data.props do
			local count = ped.getNumberOfPedPropDrawableVariations(me, slot)
			if count > 0 then
				-- -1: none.
				menu:number(name, ped.getPedPropIndex(me, slot), { min = -1, max = count - 1, note = "-1: none" }, bind.safe(name, function(p)
					if p < 0 then
						ped.clearPedProp(game.me(), slot)
					else
						ped.setPedPropIndex(game.me(), slot, p, 0, true)
					end
				end))
			end
		end
	end
	menu:onOpen(bind.safe("Wardrobe", build))
end

-- What you wear now, to put on again.
local function current(name: string): Outfit
	local me = game.me()
	local o: Outfit = { name = name, model = entity.getEntityModel(me), components = {}, props = {} }
	for slot = 0, 11 do
		o.components[slot + 1] = { ped.getPedDrawableVariation(me, slot), ped.getPedTextureVariation(me, slot) }
	end
	for slot in data.props do
		table.insert(o.props, { slot, ped.getPedPropIndex(me, slot), ped.getPedPropTextureIndex(me, slot) })
	end
	return o
end

local function wear(o: Outfit): string?
	local me = game.me()
	if entity.getEntityModel(me) ~= o.model then
		return `{o.name} is another model's outfit`
	end
	for i, c in o.components do
		ped.setPedComponentVariation(me, i - 1, c[1], c[2], 0)
	end
	for _, p in o.props do
		if p[2] < 0 then
			ped.clearPedProp(me, p[1])
		else
			ped.setPedPropIndex(me, p[1], p[2], p[3], true)
		end
	end
	return nil
end

local function outfits(root: Menu)
	local menu = root:submenu("Outfits", "Save what you wear, put it on again")
	local function saved(): { Outfit }
		return storage.get("outfits") or {}
	end
	local function build()
		menu:clear()
		menu:text("Save as", "", { placeholder = "A name, then Enter", max = 40 }, bind.safe("Save as", function(name)
			if name == "" then
				return
			end
			local list = saved()
			table.insert(list, current(name))
			storage.set("outfits", list)
			build()
			game.notify(`Saved: {name}`)
		end))
		local list = saved()
		if #list == 0 then
			menu:label("Nothing saved yet.")
			return
		end
		menu:separator("Wear")
		for _, o in list do
			bind.action(menu, o.name, { action = "Wear" }, function(): string?
				return wear(o)
			end)
		end
		local forget = menu:submenu("Delete an outfit")
		for i, o in list do
			bind.action(forget, o.name, { action = "Delete" }, function(): string?
				local l = saved()
				table.remove(l, i)
				storage.set("outfits", l)
				build()
				return `Deleted: {o.name}`
			end)
		end
	end
	build()
end

local function moves(root: Menu)
	local stop = function(): string?
		task.clearPedTasks(game.me())
		return nil
	end
	local anims = root:submenu("Animations", { search = true })
	bind.command(anims, "looks.stop", "Stop", { note = "Back to normal", action = "Stop" }, stop)
	local loopAnim = anims:toggle("Loop", true, "Over and over until stopped")
	local upper = anims:toggle("Upper body only", false, "Walk while it plays")
	anims:separator()
	for _, a in data.animations do
		local label, dict, clip = a[1], a[2], a[3]
		bind.action(anims, label, { action = "Play" }, function(): string?
			if not streaming.load(dict, "anim") then
				return `{label} is not in this game`
			end
			-- Flags: 1 loop, 16 upper body, 32 the player keeps control.
			local flags = (if loopAnim.value then 1 else 0) + (if upper.value then 48 else 0)
			task.taskPlayAnim(game.me(), dict, clip, 8, -8, -1, flags, 0, false, 0, false)
			return nil
		end)
	end

	local scenarios = root:submenu("Scenarios", { note = "What the city's people do", search = true })
	bind.action(scenarios, "Stop", { action = "Stop" }, stop)
	scenarios:separator()
	for _, s in data.scenarios do
		bind.action(scenarios, s[1], { action = "Do" }, function(): string?
			task.taskStartScenarioInPlace(game.me(), s[2], 0, true)
			return nil
		end)
	end

	local walks = root:submenu("Walk", { note = "How you walk", search = true })
	bind.action(walks, "Your own", { action = "Walk" }, function(): string?
		ped.resetPedMovementClipset(game.me(), 1)
		return nil
	end)
	walks:separator()
	for _, w in data.walks do
		bind.action(walks, w[1], { action = "Walk" }, function(): string?
			if not streaming.load(w[2], "clipSet") then
				return `{w[1]} is not in this game`
			end
			ped.setPedMovementClipset(game.me(), w[2], 1)
			return nil
		end)
	end

	local moods = {}
	for _, m in data.moods do
		table.insert(moods, m[1])
	end
	root:choice("Mood", moods, 1, "Your face", bind.safe("Mood", function(i: number)
		if i == 1 then
			ped.clearFacialIdleAnimOverride(game.me())
		else
			ped.setFacialIdleAnimOverride(game.me(), data.moods[i][2], "")
		end
	end))
end

function M.build(root: Menu)
	local menu = root:submenu("Looks", "Model, clothes, outfits, animations, walks")
	model(menu)
	wardrobe(menu)
	outfits(menu)
	menu:separator("Moves")
	moves(menu)
end

return M
