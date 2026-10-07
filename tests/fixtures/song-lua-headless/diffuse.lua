return Def.ActorFrame{
    InitCommand=function(self) self:diffusealpha(0.5) end,
    Def.Quad{
        Name="numeric",
        InitCommand=function(self)
            self:diffusealpha(0.875):diffuse(0.25, 0.5, 0.75, 0.25)
            assert(self:GetDiffuseAlpha() == 0.25)
            assert(self:GetDiffuse()[2] == 0.5)
        end,
    },
    Def.Quad{
        Name="table",
        InitCommand=function(self)
            local color = {0.25, 0.5, 0.75, 0.75}
            self:diffuse(color)
            color[4] = 0.5
            assert(self:GetDiffuseAlpha() == 0.75)
            self:diffusealpha(0.125):diffusecolor({0.5, 0.5, 0.5, 0.875})
            assert(self:GetDiffuseAlpha() == 0.125)
            self:diffuse({0.25, 0.5, 0.75, 0.5})
            assert(self:GetDiffuseAlpha() == 0.5)
        end,
    },
    Def.Quad{
        Name="tween",
        InitCommand=function(self)
            self:diffuse(0.25, 0.5, 0.75, 0.25):linear(1):diffuse({1, 1, 1, 0.75})
            assert(self:GetDiffuseAlpha() == 0.75)
        end,
    },
    Def.Quad{
        Name="transparent",
        InitCommand=function(self) self:diffuse(1, 1, 1, 0) end,
    },
    Def.Quad{
        Name="brightness",
        InitCommand=function(self)
            local brightness = PREFSMAN:GetPreference("BGBrightness")
            self:diffuse(brightness, brightness, brightness, 1)
            assert(math.abs(self:GetDiffuse()[1] - 0.7) < 0.000001)
        end,
    },
    Def.Quad{
        Name="raw",
        InitCommand=function(self)
            self:diffuse(setmetatable({0.25, 0.5, 0.75}, {__index=function() return 1 end}))
            assert(self:GetDiffuseAlpha() == 0)
        end,
    },
}
