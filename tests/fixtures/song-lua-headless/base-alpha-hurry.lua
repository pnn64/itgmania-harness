return Def.ActorFrame {
    Name = 'Parent',
    InitCommand = function(self) self:basealpha(0.5) end,
    Def.Quad {
        Name = 'Child',
        InitCommand = function(self) self:basealpha(0.5):diffuse(1, 1, 1, 1) end,
        OnCommand = function(self)
            self:linear(1):x(60):hurrytweening(0.5)
        end,
    },
}
