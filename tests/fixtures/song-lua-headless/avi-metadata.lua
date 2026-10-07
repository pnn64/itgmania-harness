return Def.ActorFrame{
    Def.Sprite{Name='Movie', Texture='avi-metadata.avi', OnCommand=function(self) self:xy(100,100):zoom(2) end},
    Def.Sprite{Name='TopDown', Texture='avi-topdown.avi', OnCommand=function(self) self:xy(200,100):zoom(2) end},
}
