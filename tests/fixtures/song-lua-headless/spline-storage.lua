return Def.ActorFrame{
    InitCommand=function(self)
        local column = SCREENMAN:GetTopScreen():GetChild("PlayerP1"):GetChild("NoteField"):GetColumnActors()[1]
        local spline = column:GetZoomHandler():GetSpline()
        assert(spline == column:GetZoomHandler():get_spline())
        assert(spline:GetSize() == 0 and spline:GetDimension() == 3)
        spline:SetSize(8.9)
        assert(spline:get_size() == 8)
        local point = {-1,-1,-1}
        spline:SetPoint(8.9, point)
        point[1] = 12
        spline:Solve()
        assert(spline:Evaluate(7)[1] == -1)
        assert(spline:Evaluate(0)[1] == 0)
        assert(not pcall(function() spline:SetPoint(9, {0,0,0}) end))
        assert(not pcall(function() spline:SetPoint(0, {0,0,0}) end))
        assert(not pcall(function() spline:SetPoint(1, false) end))
        assert(not pcall(function() spline:SetSize(-1) end))
        assert(not pcall(function() spline:Destroy() end))
        spline:set_size(3):set_size(8):solve()
        assert(spline:Evaluate(7)[1] == 0)
        spline:SetPoint(8, {"-2"}):Solve()
        local value = spline:Evaluate(7)
        assert(value[1] == -2 and value[2] == 0 and value[3] == 0)
        self:x(value[1]):y(spline:GetSize())
    end,
}
