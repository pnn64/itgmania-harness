local ticks = 0
return Def.Quad {
    InitCommand = function(self) self:zoomto(10, 10) end,
    OnCommand = function(self)
        self:queuecommand("Tick")
        self:SetUpdateFunction(function()
            if GAMESTATE:GetCurMusicSeconds() >= 1 then assert(ticks >= 59) end
        end)
    end,
    TickCommand = function(self)
        ticks = ticks + 1
        self:x(ticks):sleep(1/60):queuecommand("Tick")
    end,
}
