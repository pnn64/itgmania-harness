return Def.ActorFrame {
    Name = 'Root',
    Def.Quad {
        Name = 'AddedFirst',
        InitCommand = function(self) self:draworder(1) end,
        OnCommand = function(self) self:x(math.random(1, 2)) end,
    },
    Def.Quad {
        Name = 'DrawnFirst',
        InitCommand = function(self) self:draworder(-1) end,
        OnCommand = function(self) self:x(math.random(1, 2)) end,
    },
}
