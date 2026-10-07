local capture
return Def.ActorFrame{
    Def.ActorFrameTexture{
        Name="ActorLabel",
        InitCommand=function(self)
            capture=self
            self:SetTextureName("ResourceLabel"):SetWidth(64):SetHeight(32):Create()
        end,
    },
    Def.Sprite{
        Name="PropertyCopy", Texture="ResourceLabel",
        OnCommand=function(self)
            assert(self:GetWidth()==64 and self:GetHeight()==32)
            assert(self:GetTexture()==capture:GetTexture())
            assert(self:GetTexture():GetPath()=="ResourceLabel")
            self:xy(100,100)
        end,
    },
    Def.Sprite{
        Name="SetterCopy",
        OnCommand=function(self)
            self:SetTexture("ResourceLabel"):xy(200,100)
            assert(self:GetWidth()==64 and self:GetHeight()==32)
            assert(self:GetTexture()==capture:GetTexture())
        end,
    },
    Def.Sprite{
        Name="LoadedCopy",
        OnCommand=function(self)
            self:Load("ResourceLabel"):xy(300,100):zoomto(128,64)
            assert(self:GetWidth()==64 and self:GetHeight()==32)
            assert(self:GetTexture()==capture:GetTexture())
        end,
    },
}
