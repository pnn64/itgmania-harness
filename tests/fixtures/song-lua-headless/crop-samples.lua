local child
return Def.ActorFrame{
    InitCommand=function(self)
        self:SetUpdateFunction(function()
            if GAMESTATE:GetSongBeat() >= 0.1 then child:sleep(0):croptop(1):shadowlengthx(7):shadowlengthy(-2) end
        end)
    end,
    Def.Quad{
        Name="Crop",
        InitCommand=function(self)
            child=self
            self:zoomto(64,32):xy(100,100):cropleft(0.25):cropright(0.1):croptop(0.125):cropbottom(0.2):shadowlength(3):shadowlengthx(2):shadowcolor({0.2,0.3,0.4,0.5})
        end,
    },
}
