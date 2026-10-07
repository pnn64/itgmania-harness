return Def.Quad{
    OnCommand=function(self)
        self:SetUpdateFunction(function(self)
            if GAMESTATE:GetSongBeat() > 0.27 then
                self:diffusealpha(0):visible(false):SetUpdateFunction(nil)
            end
        end)
    end,
}
