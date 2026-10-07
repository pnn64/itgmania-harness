return Def.ActorFrame{
    Def.Quad{
        Name="positive",
        InitCommand=function(self) self:diffusealpha(math.huge) end,
    },
    Def.Quad{
        Name="negative",
        InitCommand=function(self) self:diffusealpha(-math.huge) end,
    },
    Def.Quad{
        Name="nan",
        InitCommand=function(self) self:diffusealpha(0 / 0) end,
    },
}
