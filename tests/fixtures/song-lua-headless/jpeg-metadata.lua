return Def.ActorFrame{
    Def.Sprite{
        Name='Baseline', Texture='jpeg-metadata.jpg',
        OnCommand=function(self)
            local texture = self:GetTexture()
            assert(texture:GetSourceWidth() == 64 and texture:GetSourceHeight() == 48)
            assert(texture:GetTextureWidth() == 64 and texture:GetTextureHeight() == 64)
            self:xy(100,100):zoom(2)
        end,
    },
    Def.Sprite{
        Name='ProgressiveSheet', Texture='jpeg-sheet 4x3.jpeg',
        OnCommand=function(self)
            local texture = self:GetTexture()
            assert(texture:GetSourceFrameWidth() == 16 and texture:GetSourceFrameHeight() == 16)
            assert(texture:GetNumFrames() == 12)
            self:xy(300,100):zoom(2)
        end,
    },
    Def.Actor{
        OnCommand=function()
            local path = GAMESTATE:GetCurrentSong():GetSongDir() .. 'jpeg-truncated.jpg'
            local ok = pcall(_ITG_TEXTURE_INFO, path)
            assert(not ok, 'truncated JPEG segment must fail')
        end,
    },
}
