return Def.ActorFrame {
    OnCommand = function(self)
        self:SetUpdateFunction(function()
            if GAMESTATE:GetSongBeat() >= 1 then
                while true do end
            end
        end)
    end,
}
