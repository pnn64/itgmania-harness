return Def.ActorFrame{
    Def.Quad{
        Name="Resized",
        OnCommand=function(self)
            self:x(427):y(240):zoomto(80,40):sleep(1):linear(1):zoomto(160,80)
        end,
    },
}
