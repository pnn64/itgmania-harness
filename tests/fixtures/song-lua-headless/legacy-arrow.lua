local ps = GAMESTATE:GetPlayerState(PLAYER_1)
local correction = -180 / math.pi
return Def.ActorFrame {
    Def.Quad {
        Name="LegacyArrow",
        InitCommand=function(self)
            self:rotationx(ArrowEffects.GetRotationX(ps, 0, 0, 2) + correction)
        end,
    },
    Def.Quad {
        Name="ValidArrow",
        InitCommand=function(self)
            self:rotationx(ArrowEffects.GetRotationX(ps, 0, 2))
        end,
    },
}
