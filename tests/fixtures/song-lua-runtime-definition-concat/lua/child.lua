return Def.ActorFrame {
    InitCommand = function(self)
        self:x(11)
        MESSAGEMAN:Broadcast('HarnessConcatBaseInit', {name=self:GetName(), x=self:GetX()})
    end,
    OnCommand = function(self)
        MESSAGEMAN:Broadcast('HarnessConcatOn', {name=self:GetName(), x=self:GetX(), y=self:GetY()})
    end,
}
