return Def.ActorFrame{
    Def.Sprite{Texture="coords 2x3.png"},
    Def.Sprite{Texture="coords 2x3.png", OnCommand=function(self)
        local texture = self:GetTexture()
        assert(texture:GetNumFrames() == 6)
        assert(select("#", texture:GetTextureCoordRect(5)) == 4)
        local l,t,r,b = texture:GetTextureCoordRect(5)
        assert(l == 0.4375 and t == 0.375 and r == 0.875 and b == 0.5625)
        self:Load("fit-rect.png")
        l,t,r,b = texture:GetTextureCoordRect(11)
        assert(l == 0.4375 and t == 0.375 and r == 0.875 and b == 0.5625)
        self:x(140)
    end},
}
