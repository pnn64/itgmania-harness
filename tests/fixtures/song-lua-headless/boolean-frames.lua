local options = GAMESTATE:GetPlayerState(PLAYER_1):GetPlayerOptions('ModsLevel_Song')
return Def.ActorFrame{
    InitCommand=function(self)
        self:SetUpdateFunction(function()
            options:StealthPastReceptors(true)
            options:Dark(0.25)
        end)
    end,
}
