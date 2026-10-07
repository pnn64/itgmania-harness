return Def.ActorFrame{
    InitCommand=function(self)
        self:SetUpdateFunction(function()
            if GAMESTATE:GetCurMusicSeconds() == 0 then
                local sum = 0
                for i=1,60000000 do
                    sum = sum + (i % 7) * (i % 3)
                end
                assert(sum > 0)
            end
        end)
    end,
}
