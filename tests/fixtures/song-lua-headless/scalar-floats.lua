return Def.Quad{
    InitCommand=function(self)
        self:x(.1):y(.2):z(.3):rotationx(.1):rotationy(.2):rotationz(.3)
        assert(self:GetX() == 0.10000000149011612, 'native GetX float')
        assert(self:GetY() == 0.20000000298023224, 'native GetY float')
        assert(self:GetZ() == 0.30000001192092896, 'native GetZ float')
        assert(self:GetRotationX() == 0.10000000149011612, 'native GetRotationX float')
        assert(self:GetRotationY() == 0.20000000298023224, 'native GetRotationY float')
        assert(self:GetRotationZ() == 0.30000001192092896, 'native GetRotationZ float')
        self:rotationx(100000000):addrotationx(-99999999.3)
        assert(self:GetRotationX() == 0, 'float argument conversion precedes addition')
    end
}
