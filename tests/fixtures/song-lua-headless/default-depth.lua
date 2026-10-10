return Def.ActorFrame {
    Def.Quad {
        Name = "Flat",
        InitCommand = function(self)
            self:xy(427, 240):z(25):zoomto(140, 90):linear(1):z(125)
        end,
    },
    Def.ActorFrame {
        InitCommand = function(self)
            self:xy(80, 40):rotationx(17):rotationy(-11):rotationz(7):zoom(0.8)
        end,
        Def.Quad {
            Name = "Nested",
            InitCommand = function(self)
                self:xy(150, 80):z(70):zoomto(60, 30):linear(1):z(170)
            end,
        },
    },
}
