local po = GAMESTATE:GetPlayerState(PLAYER_1):GetPlayerOptions("ModsLevel_Song")
return Def.ActorFrame{
    OnCommand = function(self)
        po:MMod(200, 7)
        po:MaxScrollBPM(0, 3)
        po:ScrollSpeed(4, 2)
        self:SetUpdateFunction(function() po:ScrollSpeed(4, 2) end)
    end,
}
