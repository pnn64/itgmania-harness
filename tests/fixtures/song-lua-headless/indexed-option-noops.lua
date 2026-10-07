local player = GAMESTATE:GetPlayerState(PLAYER_1)
local options = player:GetPlayerOptions('ModsLevel_Song')
return Def.ActorFrame{OnCommand=function(self)
    options:FromString('*10000 70% confusionoffset, *10000 25% movex1, *10000 -50% movey4')
    local before = player:GetPlayerOptionsString('ModsLevel_Song')
    options:FromString('*10000 900% confusionoffset0, *10000 900% movex0, *10000 900% movey0, *10000 900% movex17')
    assert(before == player:GetPlayerOptionsString('ModsLevel_Song'), 'native invalid columns changed options')
end}
