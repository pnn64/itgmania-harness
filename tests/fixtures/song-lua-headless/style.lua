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
local screen = SCREENMAN:GetTopScreen()
local player = screen:GetChild("PlayerP1")
local other = screen:GetChild("PlayerP2")
assert((other ~= nil) == not double)
assert((screen:GetChildren().PlayerP2 ~= nil) == not double)
-- A failed lookup must not synthesize an actor on subsequent queries.
assert(screen:GetChild("PlayerP2") == other)
-- Observed with the pinned native engine (Simply Love, single / versus).
-- Empty and hidden screen children affect pairs() order just like drawables.
local names = {}
local children = screen:GetChildren()
for name in pairs(children) do
    if name ~= "" then names[#names + 1] = name end
end
local expected = double
    and "SongOptions,Cancel,SongForeground,OniGameOverP1,ScoreP1,LyricDisplay,ActiveAttackListP2,PlayerP1,Debug,LifeP1,Out,In,ActiveAttackListP1,Underlay,StepsDisplayP1,Overlay,Header,Footer,StepsDescriptionP1,PlayerOptionsP1,SongBackground"
    or "Cancel,SongForeground,OniGameOverP1,ScoreP1,Out,In,LifeP2,Debug,ScoreP2,Overlay,ActiveAttackListP2,PlayerOptionsP1,LyricDisplay,PlayerP1,PlayerOptionsP2,LifeP1,ActiveAttackListP1,SongOptions,StepsDescriptionP2,Underlay,StepsDisplayP1,SongBackground,Header,Footer,StepsDescriptionP1,PlayerP2,OniGameOverP2,StepsDisplayP2"
assert(table.concat(names, ",") == expected, table.concat(names, ","))
assert(#children[""] == 5)
assert(screen:GetChild("MissingChild") == nil)
assert(screen:GetChild("Header"):GetVisible())
assert(not screen:GetChild("PlayerOptionsP1"):GetVisible())
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
