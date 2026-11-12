return Def.ActorFrame {
    Name = 'Root',
    OnCommand = function(self) self:sleep(12) end,
    Def.Quad {
        Name = 'Heading',
        InitCommand = function(self)
            self:setsize(20, 10):heading(90)
        end,
        OnCommand = function(self)
            self:linear(1):heading(-90)
        end,
    },
    Def.Quad {
        Name = 'Pitch',
        InitCommand = function(self)
            self:setsize(20, 10):pitch(90)
        end,
    },
    Def.Quad {
        Name = 'Roll',
        InitCommand = function(self)
            self:setsize(20, 10):roll(90)
        end,
    },
}
