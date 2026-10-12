return Def.ActorFrame {
    LoadActor('child.lua') .. {
        Name = 'Composed',
        InitCommand = function(self)
            self:y(22)
            MESSAGEMAN:Broadcast('HarnessConcatAddedInit', {name=self:GetName(), x=self:GetX(), y=self:GetY()})
        end,
        ExtraMessageCommand = function(self) self:aux(3) end,
    },
}
