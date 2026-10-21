--!strict
-- Draw: an image of the mod's own, turning, with a caption over a panel.

local logo = draw.loadImage("images/logo.png")
local width, height = draw.screenSize()
print("logo", logo.width, logo.height, "on a screen of", width, height)

local turn = 0
while true do
	turn += 1
	draw.rect(0.5, 0.86, 0.6, 0.1, { color = { 0, 0, 0, 170 } })
	-- No height: the image's own proportions.
	draw.image(logo, 0.2, 0.35, 0.2)
	draw.image(logo, 0.8, 0.35, 0.15, 0.3, { rotation = turn, color = { 255, 255, 255, 140 }, z = 1 })
	draw.text("GLUE 5 draw test", 0.5, 0.83, { size = 0.05, align = "center", color = { 255, 220, 0 }, z = 2 })
	wait()
end
