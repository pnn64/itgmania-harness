local root = Def.ActorFrame{}
for index, path in ipairs({"mp4-aspect.mp4", "mp4-aspect-faststart.mp4"}) do
    local center_x = index * 100
    root[#root + 1] = Def.Sprite{
        Texture = path,
        InitCommand = function(self)
            assert(self:GetWidth() == 16 and self:GetHeight() == 16,
                string.format("source size %gx%g", self:GetWidth(), self:GetHeight()))
            local texture = self:GetTexture()
            assert(texture:GetSourceWidth() == 16 and texture:GetSourceHeight() == 16)
            self:xy(center_x, 100):zoomtowidth(32)
        end,
    }
end
return root
