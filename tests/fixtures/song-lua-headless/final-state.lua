local target
return Def.ActorFrame{
    Def.Quad{
        Name="FinalState",
        InitCommand=function(self) target=self end,
    },
    Def.ActorFrame{
        OnCommand=function(self)
            self:SetUpdateFunction(function()
                if GAMESTATE:GetSongBeat() > 0.27 then
                    target:diffusealpha(0):visible(false)
                    self:SetUpdateFunction(nil)
                end
            end)
        end,
    },
}
