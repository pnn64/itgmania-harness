return Def.ActorFrame {
  OnCommand=function(self)
    self:hibernate(0/0):SetUpdateRate(2)
    for _,v in ipairs{0, -1, -math.huge} do
      local ok,err=pcall(function() self:SetUpdateRate(v) end)
      assert(not ok, "nonpositive SetUpdateRate must raise")
      assert(err:find("Update rate must be greater than 0.", 1, true), "native binding error text")
      assert(self:GetUpdateRate()==2, "rejected rate changed state")
    end
    assert(pcall(function() self:SetUpdateRate(0/0) end), "NaN binding must return")
    assert(self:GetUpdateRate()==2, "native C++ setter ignores NaN")
    local zero_seen=false
    self:SetUpdateFunction(function(_,delta)
      if delta==0 then zero_seen=true else assert(zero_seen, "NaN hibernation skipped Update(0)") end
      assert(delta==0 or math.abs(delta-2/60)<1e-6, "callback uses changed rate")
    end)
  end,
}
