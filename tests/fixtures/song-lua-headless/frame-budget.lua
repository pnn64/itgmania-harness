return Def.Actor {
    OnCommand = function(self)
        self:SetUpdateFunction(function(actor)
            local total = 0
            for index = 1, 1000000 do total = total + index end
            actor:aux(total)
        end)
    end,
}
