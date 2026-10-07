return Def.ActorFrame{
    OnCommand=function(self)
        self:SetUpdateFunction(function(frame)
            for _, name in ipairs({'Timer', 'Music'}) do
                local actor = frame:GetChild(name)
                actor:xy(actor:GetSecsIntoEffect(), actor:GetEffectDelta())
            end
        end)
    end,
    Def.Quad{Name='Timer', OnCommand=cmd(spin;effectmagnitude,0,0,0;effectperiod,1.5)},
    Def.Quad{Name='Music', OnCommand=cmd(spin;effectmagnitude,0,0,0;effectperiod,1.5;effectclock,'music')},
}
