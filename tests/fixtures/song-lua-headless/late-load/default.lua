return Def.Quad{
    OnCommand=function(self) self:sleep(0.1):queuecommand("Load") end,
    LoadCommand=function(self)
        local actor = LoadActor("helper.lua")
        assert(actor.Name == "Late helper")
        self:x(actor.Value)
    end,
}
