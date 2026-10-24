--!strict
-- You: health and protection, the police, abilities, sight.

local bind = require("../core/bind")
local game = require("../core/game")
local loop = require("../core/loop")
local prefs = require("../core/prefs")

local M = {}

-- What SET_ENTITY_PROOFS protects from, in its order.
local PROOFS = { "Bullets", "Fire", "Explosions", "Collisions", "Melee", "Steam", "Drowning" }

-- The proofs chosen, every frame while any is (missions reset them).
local function proofs(root: Menu)
	local item: Multi
	local function apply()
		local on = {}
		for _, i in item.value do
			on[i] = true
		end
		entity.setEntityProofs(game.me(), on[1] == true, on[2] == true, on[3] == true, on[4] == true, on[5] == true, on[6] == true, false, on[7] == true)
	end
	local function follow()
		prefs.set("self.proofs", item.value)
		if #item.value > 0 then
			loop.add("self.proofs", apply)
		else
			loop.remove("self.proofs")
			apply()
		end
	end
	item = root:multi("Proofs", PROOFS, prefs.get("self.proofs", {}), "Hurt by everything but these", bind.safe("Proofs", follow))
	if #item.value > 0 then
		follow()
	end
end

local function health(root: Menu)
	root:separator("Health")
	bind.feature(root, "self.god", "God mode", "Nothing hurts you", {
		-- Every frame: missions switch it off, and a new model is a new ped.
		tick = function()
			player.setPlayerInvincible(game.player(), true)
		end,
		off = function()
			player.setPlayerInvincible(game.player(), false)
		end,
	})
	proofs(root)
	bind.slider(root, "self.regen", "Health regeneration", 1, 0, 10, { step = 0.5, unit = "×", note = "How fast health comes back" }, function(v)
		player.setPlayerHealthRechargeMultiplier(game.player(), v)
	end)
	-- Invincible only while falling (God mode, when on, keeps it).
	local falling = false
	bind.feature(root, "self.noFall", "No fall damage", "Land from any height", {
		tick = function()
			local me = game.me()
			local now = ped.isPedFalling(me) or ped.isPedInParachuteFreeFall(me)
			if now ~= falling and not bind.on("self.god") then
				player.setPlayerInvincible(game.player(), now)
			end
			falling = now
		end,
		off = function()
			if falling and not bind.on("self.god") then
				player.setPlayerInvincible(game.player(), false)
			end
			falling = false
		end,
	})
	bind.feature(root, "self.noCritical", "No headshots", "No critical hits", {
		tick = function()
			ped.setPedSuffersCriticalHits(game.me(), false)
		end,
		off = function()
			ped.setPedSuffersCriticalHits(game.me(), true)
		end,
	})
	bind.feature(root, "self.ragdoll", "No ragdoll", "Never knocked over", {
		tick = function()
			ped.setPedCanRagdoll(game.me(), false)
			ped.setPedCanRagdollFromPlayerImpact(game.me(), false)
		end,
		off = function()
			ped.setPedCanRagdoll(game.me(), true)
			ped.setPedCanRagdollFromPlayerImpact(game.me(), true)
		end,
	})
	bind.command(root, "self.heal", "Heal", { note = "Full health and armour", action = "Heal" }, function(): string?
		local me = game.me()
		entity.setEntityHealth(me, entity.getEntityMaxHealth(me), nil)
		ped.setPedArmour(me, player.getPlayerMaxArmour(game.player()))
		return "Healed"
	end)
	bind.command(root, "self.clean", "Clean up", { note = "Blood, dirt and water off", action = "Clean" }, function(): string?
		local me = game.me()
		ped.clearPedBloodDamage(me)
		ped.resetPedVisibleDamage(me)
		ped.clearPedWetness(me)
		return nil
	end)
	bind.command(root, "self.die", "Die", { action = "Die" }, function(): string?
		entity.setEntityHealth(game.me(), 0, nil)
		return nil
	end)
end

local function police(root: Menu)
	root:separator("Police")
	bind.feature(root, "self.neverWanted", "Never wanted", "Stars go as soon as they come", {
		tick = function()
			local me = game.player()
			if player.getPlayerWantedLevel(me) > 0 then
				player.clearPlayerWantedLevel(me)
			end
		end,
	})
	local wanted = root:number("Wanted level", 0, { min = 0, max = 5, note = "Stars, now" }, bind.safe("Wanted level", function(stars)
		local me = game.player()
		player.setPlayerWantedLevel(me, stars, false)
		player.setPlayerWantedLevelNow(me, false)
	end))
	root:onOpen(bind.safe("Self", function()
		wanted.value = player.getPlayerWantedLevel(game.player())
	end))
	bind.feature(root, "self.ignored", "Ignored by everyone", "Police and gangs leave you be", {
		tick = function()
			player.setEveryoneIgnorePlayer(game.player(), true)
			player.setPoliceIgnorePlayer(game.player(), true)
		end,
		off = function()
			player.setEveryoneIgnorePlayer(game.player(), false)
			player.setPoliceIgnorePlayer(game.player(), false)
		end,
	})
end

local function abilities(root: Menu)
	root:separator("Abilities")
	bind.feature(root, "self.superJump", "Super jump", "Jump over houses", {
		tick = function()
			misc.setSuperJumpThisFrame(game.player())
		end,
	})
	bind.feature(root, "self.superRun", "Super run", "Sprint as fast as a car", {
		tick = function()
			local me = game.me()
			if pad.isControlPressed(0, Control.SPRINT) and not ped.isPedInAnyVehicle(me, false) and not entity.isEntityInAir(me) then
				local ahead = entity.getEntityForwardVector(me) * 25
				entity.setEntityVelocity(me, vector.create(ahead.x, ahead.y, entity.getEntityVelocity(me).z))
			end
		end,
	})
	-- The game takes 1 to 1.49 (more is ignored).
	bind.slider(root, "self.run", "Run speed", 1, 1, 1.49, { step = 0.01, unit = "×" }, function(v)
		player.setRunSprintMultiplierForPlayer(game.player(), v)
	end)
	bind.slider(root, "self.swim", "Swim speed", 1, 1, 1.49, { step = 0.01, unit = "×" }, function(v)
		player.setSwimMultiplierForPlayer(game.player(), v)
	end)
	bind.feature(root, "self.stamina", "Unlimited stamina", "Sprint and swim for ever", {
		tick = function()
			player.restorePlayerStamina(game.player(), 1)
		end,
	})
	bind.feature(root, "self.special", "Unlimited special ability", "A story character's, always full", {
		tick = function()
			player.specialAbilityFillMeter(game.player(), true, 0)
		end,
	})
	bind.feature(root, "self.quiet", "Noiseless", "Nobody hears you", {
		tick = function()
			player.setPlayerNoiseMultiplier(game.player(), 0)
		end,
		off = function()
			player.setPlayerNoiseMultiplier(game.player(), 1)
		end,
	})
	bind.feature(root, "self.parachute", "Always a parachute", "One on your back whenever you have none", {
		tick = function()
			local me = game.me()
			local chute = joaat("GADGET_PARACHUTE")
			if not weapon.hasPedGotWeapon(me, chute, 0) then
				weapon.giveWeaponToPed(me, chute, 1, false, false)
			end
		end,
	})
	bind.feature(root, "self.invisible", "Invisible", "Others do not see you; you still do", {
		tick = function()
			entity.setEntityVisible(game.me(), false, false)
			network.setEntityLocallyVisible(game.me())
		end,
		off = function()
			entity.setEntityVisible(game.me(), true, false)
		end,
	})
	bind.feature(root, "self.burn", "On fire", "Burning, and fireproof", {
		on = function()
			entity.setEntityProofs(game.me(), false, true, false, false, false, false, false, false)
			fire.startEntityFire(game.me())
		end,
		off = function()
			fire.stopEntityFire(game.me())
			entity.setEntityProofs(game.me(), false, false, false, false, false, false, false, false)
		end,
	})
end

local function sight(root: Menu)
	root:separator("Sight")
	bind.feature(root, "self.nightVision", "Night vision", nil, {
		on = function()
			graphics.setNightvision(true)
		end,
		off = function()
			graphics.setNightvision(false)
		end,
	})
	bind.feature(root, "self.thermal", "Thermal vision", nil, {
		on = function()
			graphics.setSeethrough(true)
		end,
		off = function()
			graphics.setSeethrough(false)
		end,
	})
	bind.feature(root, "self.drunk", "Drunk", "Walk, sway and see as after a night out", {
		on = function()
			streaming.load("move_m@drunk@verydrunk", "clipSet")
			ped.setPedMovementClipset(game.me(), "move_m@drunk@verydrunk", 1)
			camera.shakeGameplayCam("DRUNK_SHAKE", 1)
			graphics.setTimecycleModifier("Drunk")
		end,
		off = function()
			ped.resetPedMovementClipset(game.me(), 1)
			camera.stopGameplayCamShaking(true)
			graphics.clearTimecycleModifier()
		end,
	})
end

function M.build(root: Menu)
	local me = root:submenu("Self", "Health, protection, police, abilities, sight")
	health(me)
	police(me)
	abilities(me)
	sight(me)
end

return M
