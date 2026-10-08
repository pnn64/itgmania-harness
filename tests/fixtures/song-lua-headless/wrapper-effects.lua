return Def.ActorFrame {
    Name = "Root",
    InitCommand = function(self) self:xy(100,80):vibrate():effectmagnitude(1,2,0) end,
    Def.Quad {
        Name = "Wrapped",
        InitCommand = function(self)
            self:setsize(40,20):xy(10,5)
            self:AddWrapperState():vibrate():effectmagnitude(5,7,0)
            self:AddWrapperState():vibrate():effectmagnitude(19,23,0)
        end,
    },
}
