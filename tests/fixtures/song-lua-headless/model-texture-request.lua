local piece = GAMESTATE:GetCurrentSong():GetSongDir() .. "model-texture-request/model.txt"
return Def.Model { Name="Requested", Meshes=piece, Materials=piece, Bones=piece,
    InitCommand=function(self) self:glow(1,1,1,0.25) end }
