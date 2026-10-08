local cover
return Def.ActorFrame{
    Def.ActorFrame{
        InitCommand=function(self) self:Center() end,
        Def.Sprite{
            Name="Fit",
            Texture="fit-rect.png",
            InitCommand=function(self)
                assert(self:GetWidth() == 64 and self:GetHeight() == 32)
                self:xy(17, 19):scale_or_crop_background_no_move()
                assert(self:GetX() == 17 and self:GetY() == 19)
                assert(self:GetZoomX() == 15 and self:GetZoomY() == 15)
                self:scale_or_crop_background()
                assert(self:GetX() == SCREEN_CENTER_X and self:GetY() == SCREEN_CENTER_Y)
                self:align(.5, 0):xy(0, -SCREEN_HEIGHT/2)
            end,
        },
    },
    Def.Quad{
        Name="Cover",
        InitCommand=function(self)
            cover = self
            self:FullScreen():diffusealpha(0)
        end,
        FadeMessageCommand=function(self) self:sleep(1):smooth(2):diffusealpha(1) end,
    },
    Def.ActorFrame{
        Name="CoverDriver",
        OnCommand=function(self)
            local fired = false
            self:SetUpdateFunction(function()
                if not fired and GAMESTATE:GetSongBeat() > 0 then
                    cover:sleep(1):smooth(2):diffusealpha(1)
                    fired = true
                end
            end)
        end,
    },
}
