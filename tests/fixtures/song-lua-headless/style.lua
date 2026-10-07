local double = GAMESTATE:GetCurrentSteps(PLAYER_1):GetStepsType() == "StepsType_Dance_Double"
local style = GAMESTATE:GetCurrentStyle()
local count = double and 8 or 4
assert(style:GetName() == (double and "double" or "single"))
assert(style:GetColsPerPlayer() == count)
assert(style:ColumnsPerPlayer() == count)
assert(style:GetWidth() == count * 64)
assert(#GAMESTATE:GetEnabledPlayers() == (double and 1 or 2))
assert(#GAMESTATE:GetHumanPlayers() == (double and 1 or 2))
assert(GAMESTATE:IsPlayerEnabled(PLAYER_1))
assert(GAMESTATE:IsPlayerEnabled(PLAYER_2) == not double)
assert(GAMESTATE:IsHumanPlayer(1) == not double)
local player = SCREENMAN:GetTopScreen():GetChild("PlayerP1")
local columns = player:GetChild("NoteField"):GetColumnActors()
assert(#columns == count)
assert(columns[count])
local options = GAMESTATE:GetPlayerState(PLAYER_1):GetPlayerOptions("ModsLevel_Song")
for _, name in ipairs({"Mirror", "Left", "Right", "NoMines"}) do
	assert(options[name](options) == false)
	options[name](options, true)
	assert(options[name](options) == true)
	options[name](options, false)
	assert(options[name](options) == false)
end
if double then assert(player:GetX() == SCREEN_CENTER_X) end
return Def.ActorFrame { Name = "style-verified" }
