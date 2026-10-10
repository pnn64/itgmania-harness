-- GameState modifier queries and ArrowEffects read Current, not Song targets.
local po = GAMESTATE:GetPlayerState(PLAYER_1):GetCurrentPlayerOptions()
local function uses(text, player)
    return GAMESTATE:PlayerIsUsingModifier(player or PLAYER_1, text)
end
assert(uses('1x', 0) and uses('1x', 1))
local detected = 2.5
for i=0,10,0.01 do
    if uses((math.floor(i * 100 + 0.5) / 100) .. 'x', 0) then detected = i; break end
end
assert(math.abs(detected - 1) < 0.000001)
assert(uses("no reverse") and not uses("reverse"))
po:Reverse(0.5)
assert(uses("*999 50% reverse") and not uses("reverse"))
assert(uses("reverse, no reverse, 50% reverse"))
assert(uses("no reverse", PLAYER_2))
assert(uses("unknown-modifier"))
po:XMod(2)
assert(uses("2x") and not uses("1x"))
po:CMod(500)
assert(uses("C500") and not uses("2x"))
po:XMod(2)
assert(uses("2x") and not uses("C500"))
GAMESTATE:GetSongOptionsObject("ModsLevel_Current"):MusicRate(1.5)
assert(uses("1.5xmusic") and not uses("1xmusic"))
GAMESTATE:GetPlayerState(PLAYER_2):SetPlayerOptions("ModsLevel_Stage", "reverse")
assert(uses("reverse", PLAYER_2) and not uses("no reverse", PLAYER_2))
local ps = GAMESTATE:GetPlayerState(PLAYER_2)
assert(ps:GetPlayerOptionsString("ModsLevel_Song"):find("Reverse", 1, true))
ps:SetPlayerOptions("ModsLevel_Stage", "50% drunk")
assert(uses("no reverse", PLAYER_2) and uses("50% drunk", PLAYER_2))
local serialized = ps:GetPlayerOptionsString("ModsLevel_Song")
assert(serialized:find("50% Drunk", 1, true) and not serialized:find("Reverse", 1, true))
ps:SetPlayerOptions("ModsLevel_Stage", serialized .. ", 25% mini")
assert(uses("50% drunk, 25% mini", PLAYER_2))
assert(not ps:GetPlayerOptionsString("ModsLevel_Song"):find("*", 1, true))
ps:SetPlayerOptions("ModsLevel_Stage", "clearall")
assert(uses("no drunk, no mini, 1x", PLAYER_2))
po:FromString("clearall")
local p1 = GAMESTATE:GetPlayerState(PLAYER_1)
assert(ArrowEffects.GetYPos(p1, 1, 32) == -103)
po:Reverse(0.5)
assert(ArrowEffects.GetYPos(p1, 1, 32) == 0)
po:Reverse(1)
po:Mini(0.5)
assert(ArrowEffects.GetYPos(p1, 1, 32) == 148)
po:Centered(0.5)
assert(ArrowEffects.GetYPos(p1, 1, 32) == 58)
assert(ArrowEffects.GetYPos(p1, 1, 32, 360) == 88)
po:FromString("clearall")
po:FromString("25% Reverse, 50% Split, 12.5% Alternate, 25% Cross, 25% Reverse1")
for col, expected in ipairs({0.5, 0.625, 1, 0.875}) do
    assert(po:GetReversePercentForColumn(col - 1) == expected)
end
assert(po:GetReversePercentForColumn(-1) == nil and po:GetReversePercentForColumn(5) == nil)
po:FromString("clearall, 175% Reverse")
assert(po:GetReversePercentForColumn(0) == 0.25)
po:FromString("clearall, -50% Reverse")
assert(po:GetReversePercentForColumn(0) == -0.5)
assert(ArrowEffects.GetYPos(p1, 1, 32) == -206)
po:FromString("clearall")
assert(ProductVersion():match("^%d+%.%d+%.%d+"))
assert(IsMinimumProductVersion(1) and not IsMinimumProductVersion(999))
assert(WideScale(100, 200) == 200)
assert(GAMESTATE:GetNumPlayersEnabled() == 2)
assert(THEME:GetMetric("Common", "ScreenHeight") == 480)
for _, style in ipairs({"OnePlayerOneSide", "TwoPlayersTwoSides"}) do
    assert(THEME:HasMetric("ScreenGameplay", "PlayerP1" .. style .. "X"))
    assert(THEME:HasMetric("ScreenGameplay", "PlayerP2" .. style .. "X"))
    assert(THEME:GetMetric("ScreenGameplay", "PlayerP1" .. style .. "X") == 213.5)
    assert(THEME:GetMetric("ScreenGameplay", "PlayerP2" .. style .. "X") == 640.5)
end
for _, style in ipairs({"OnePlayerBothSides", "OnePlayerTwoSides", "TwoPlayersSharedSides"}) do
    assert(THEME:GetMetric("ScreenGameplay", "PlayerP1" .. style .. "X") == 427)
    assert(THEME:GetMetric("ScreenGameplay", "PlayerP2" .. style .. "X") == 427)
end
assert(not THEME:HasMetric("ScreenGameplay", "PlayerP1NoSuchStyleX"))
assert(THEME:HasMetric("Player", "ReceptorArrowsYStandard"))
assert(THEME:HasMetric("Player", "ReceptorArrowsYReverse"))
assert(THEME:GetMetric("Player", "ReceptorArrowsYStandard") == -125)
assert(THEME:GetMetric("Player", "ReceptorArrowsYReverse") == 145)
assert(THEME:HasMetric("Player", "DrawDistanceBeforeTargetsPixels"))
assert(THEME:HasMetric("Player", "DrawDistanceAfterTargetsPixels"))
local front = THEME:GetMetric("Player", "DrawDistanceBeforeTargetsPixels")
local back = THEME:GetMetric("Player", "DrawDistanceAfterTargetsPixels")
assert(front == 720 and back == -130)
po:FromString(string.format("*9999 %.9g drawsize", (960 / front - 1) * 100))
assert(math.abs(po:DrawSize() - 1/3) < 0.000001)
po:DrawSize(-1.5, 2, true):DrawSizeBack(-1, 4, true)
local amount, speed = po:DrawSize()
local back_amount, back_speed = po:DrawSizeBack()
assert(amount == -1.5 and speed == 2 and back_amount == -1 and back_speed == 4)
po:FromString("no drawsize,no drawsizeback")
assert(po:DrawSize() == 0 and po:DrawSizeBack() == 0)
assert(SONGMAN:FindSong("song-lua-headless") == GAMESTATE:GetCurrentSong())
assert(SONGMAN:FindSong("missing/song") == nil)
local field = SCREENMAN:GetTopScreen():GetChild("PlayerP1"):GetChild("NoteField")
local columns = field:get_column_actors()
assert(#columns == 4)
assert(columns[1]:get_rot_handler() == columns[1]:GetRotHandler())
assert(columns[1]:get_pos_handler() == columns[1]:GetPosHandler())
assert(columns[1]:get_zoom_handler() == columns[1]:GetZoomHandler())
assert(columns[1]:get_rot_handler():get_spline() == columns[1]:GetRotHandler():GetSpline())
columns[1]:get_rot_handler():get_spline():set_size(2)
setenv("missionmode", "test")
assert(GAMESTATE:Env().missionmode == "test")
setenv("missionmode", nil)
assert(getenv("missionmode") == nil)
-- Use the linked ITGmania getters, including inactive nils and both speeds.
po:Overhead(true)
assert(po:Overhead() == true and select('#', po:Overhead()) == 1)
for _, name in ipairs{'Incoming', 'Space', 'Hallway', 'Distant'} do
    assert(po[name](po) == nil and select('#', po[name](po)) == 2)
end
assert(po:Space(0.5, 3, true) == po)
assert(po:Incoming() == nil and po:Tilt() == 0.5 and po:Skew() == 0.5)
local old, speed = po:Space(-0.25, 4)
assert(old == 0.5 and speed == 3 and po:Space() == -0.25)
assert(po:Overhead(false, 7, true) == po and po:Tilt() == -0.25)
assert(select(2, po:Tilt()) == 7 and select(2, po:Skew()) == 7)
po:Incoming(0.75)
assert(po:Incoming() == 0.75 and po:Space() == nil and po:Tilt() == -0.75)
po:Hallway(-0.5)
assert(po:Hallway() == nil and po:Distant() == 0.5 and po:Skew() == 0)
po:Overhead(0)
assert(po:Overhead() == true)
po:FromString('*9 50% incoming')
assert(po:Incoming() == 0.5 and select(2, po:Incoming()) == 9)
po:FromString('no overhead')
assert(po:Overhead() == true)

-- Execute native Bumpy and Drunk bindings, including independent speeds.
local wave_names = {"ModTimerMult", "ModTimerOffset", "BumpyX","BumpyXOffset","BumpyXPeriod","TanBumpy","TanBumpyOffset","TanBumpyPeriod","TanBumpyX","TanBumpyXOffset","TanBumpyXPeriod","DrunkZ", "DrunkZOffset", "DrunkZSpeed", "DrunkZPeriod", "TanDrunk", "TanDrunkOffset", "TanDrunkSpeed", "TanDrunkPeriod", "TanDrunkZ", "TanDrunkZOffset", "TanDrunkZSpeed", "TanDrunkZPeriod"}
for i, name in ipairs(wave_names) do
    local amount, speed = po[name](po)
    assert(amount == 0 and speed == 1 and select('#', po[name](po)) == 2)
    assert(po[name](po, -i/4, i/2, true) == po)
    amount, speed = po[name](po)
    assert(amount == -i/4 and speed == i/2)
    local old, old_speed = po[name](po, i/8)
    assert(old == -i/4 and old_speed == i/2 and select(2, po[name](po)) == i/2)
    local ok = pcall(function() po[name](po, -i/8, -1) end)
    assert(not ok and po[name](po) == -i/8)
    po:FromString('no '..string.lower(name))
    assert(po[name](po) == 0 and select(2, po[name](po)) == 1)
end
assert(po:Cosecant() == false and select('#', po:Cosecant()) == 1)
assert(po:Cosecant(true) == false and po:Cosecant() == true)
assert(po:Cosecant(0) == true and po:Cosecant() == true)
assert(po:Cosecant(false, false) == po and po:Cosecant() == false)
po:FromString('51% cosecant'); assert(po:Cosecant() == true)
po:FromString('50% cosecant'); assert(po:Cosecant() == false)

-- Native BOOL_INTERFACE: one previous boolean; only a boolean first arg writes.
assert(po:DizzyHolds() == false and select('#', po:DizzyHolds()) == 1)
assert(po:DizzyHolds(true) == false and po:DizzyHolds() == true)
for _, invalid in ipairs({0, 1, 'true', 'false'}) do
    assert(po:DizzyHolds(invalid) == true and po:DizzyHolds() == true)
end
assert(po:DizzyHolds(true, false) == po)
assert(po:DizzyHolds(false, 0, true) == true and po:DizzyHolds() == false)
assert(po:DizzyHolds(true, true) == po)
po:FromString('*0 50% dizzyholds'); assert(po:DizzyHolds() == false)
po:FromString('*0 51% dizzyholds'); assert(po:DizzyHolds() == true)
po:FromString('no dizzyholds'); assert(po:DizzyHolds() == false)

-- Native fade-coordinate switch also follows BOOL_INTERFACE, with no speed.
assert(po:StealthType() == false and select('#', po:StealthType()) == 1)
assert(po:StealthType(true) == false and po:StealthType() == true)
for _, invalid in ipairs({0, 1, 'true', 'false'}) do
    assert(po:StealthType(invalid) == true and po:StealthType() == true)
end
assert(po:StealthType(nil, false) == po)
assert(po:StealthType(false, 0, true) == true and po:StealthType() == false)
assert(po:StealthType(true, true) == po)
po:FromString('*0 50% stealthtype'); assert(po:StealthType() == false)
po:FromString('*0 51% stealthtype'); assert(po:StealthType() == true)
assert(uses('stealthtype') and not uses('no stealthtype'))
po:FromString('no stealthtype'); assert(po:StealthType() == false)
po:StealthType(true)
po:FromString('clearall'); assert(po:StealthType() == false)

-- Explicit hold depth follows the native immediate boolean protocol.
assert(po:ZBuffer() == false and select('#', po:ZBuffer()) == 1)
assert(po:ZBuffer(true) == false and po:ZBuffer() == true)
for _, invalid in ipairs({0, 1, 'true', 'false'}) do
    assert(po:ZBuffer(invalid) == true and po:ZBuffer() == true)
end
assert(po:ZBuffer(nil, false) == po)
assert(po:ZBuffer(false, 0, true) == true and po:ZBuffer() == false)
assert(po:ZBuffer(true, true) == po)
po:FromString('*0 50% zbuffer'); assert(po:ZBuffer() == false)
po:FromString('*0 51% zbuffer'); assert(po:ZBuffer() == true)
assert(uses('zbuffer') and not uses('no zbuffer'))
po:FromString('no zbuffer'); assert(po:ZBuffer() == false)
po:ZBuffer(true)
po:FromString('clearall'); assert(po:ZBuffer() == false)

-- Every native boolean binding shares the same argument and return protocol.
for _, name in ipairs({"StealthType", "StealthPastReceptors", "DizzyHolds", "ZBuffer", "Cosecant", "TurnNone", "Mirror", "LRMirror", "UDMirror", "Backwards", "Left", "Right", "Shuffle", "SoftShuffle", "SuperShuffle", "HyperShuffle", "NoHolds", "NoRolls", "NoMines", "Little", "Wide", "Big", "Quick", "BMRize", "Skippy", "Mines", "AttackMines", "Echo", "Stomp", "Planted", "Floored", "Twister", "HoldRolls", "NoJumps", "NoHands", "NoLifts", "NoFakes", "NoQuads", "NoStretch", "MuteOnError"}) do
    local method = po[name]
    assert(method(po, false, true) == po)
    assert(method(po) == false and select('#', method(po)) == 1)
    assert(method(po, true) == false and method(po) == true)
    for _, invalid in ipairs({0, 1, 'true', 'false', {}}) do
        assert(method(po, invalid) == true and method(po) == true)
    end
    assert(method(po, nil, false) == po and method(po) == true)
    assert(method(po, false, 0, true) == true and method(po) == false)
    assert(method(po, true, -1) == false and method(po) == true)
    assert(method(po, false, false) == po and method(po) == false)
end

-- Execute the linked enum binding: previous value, nil query, and true chain.
assert(ModTimerType:GetName() == 'ModTimerType')
assert(ModTimerType:Reverse()['song'] == 2 and ModTimerType:Reverse()[0] == 0)
assert(po:ModTimerSetting() == 'ModTimerType_Default' and select('#', po:ModTimerSetting()) == 1)
assert(po:ModTimerSetting(ModTimerType[3]) == 'ModTimerType_Default')
assert(po:ModTimerSetting() == 'ModTimerType_Song')
assert(po:ModTimerSetting('GaMe', true) == po and po:ModTimerSetting() == 'ModTimerType_Game')
assert(po:ModTimerSetting(1) == 'ModTimerType_Game' and po:ModTimerSetting() == 'ModTimerType_Beat')
assert(po:ModTimerSetting(nil) == 'ModTimerType_Beat')
assert(po:ModTimerSetting(nil, true) == po)
for _, invalid in ipairs({-1, 4, 1.5, '1', 'modtimertype_song', false}) do
    assert(not pcall(function() po:ModTimerSetting(invalid) end))
    assert(po:ModTimerSetting() == 'ModTimerType_Beat')
end
for _, mode in ipairs({'Game', 'Beat', 'Song', 'Default'}) do
    po:FromString('*0 no modtimer'..mode)
    assert(po:ModTimerSetting() == 'ModTimerType_'..mode)
    assert(uses('modtimer'..mode) and not uses('modtimer'..(mode == 'Song' and 'Game' or 'Song')))
end
po:ModTimerMult(1, 7, true):ModTimerOffset(-2, 8, true)
po:FromString('clearall')
assert(po:ModTimerSetting() == 'ModTimerType_Default')
assert(po:ModTimerMult() == 0 and select(2, po:ModTimerMult()) == 1)
assert(po:ModTimerOffset() == 0 and select(2, po:ModTimerOffset()) == 1)

return Def.Quad{InitCommand=function(self)
    bg_fit_functions.BackgroundFitMode_CoverDistort(self, 512, 256)
    -- Actor/Quad starts at 1x1, as initialized by native Actor.cpp.
    assert(self:GetZoomX() == 512 and self:GetZoomY() == 256)
    self:x(42)
end}
