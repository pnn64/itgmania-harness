local actors = Def.ActorFrame{}
for index = 1, 400 do
    actors[#actors + 1] = Def.Quad{
        Name = string.format('tile:%d\n"\\', index),
        InitCommand = function(self) self:xy(index, 100):zoomto(4, 8) end,
    }
end
return actors
