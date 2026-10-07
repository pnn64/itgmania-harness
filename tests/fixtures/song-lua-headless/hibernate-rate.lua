local callbacks = 0
return Def.ActorFrame {
  OnCommand = function(self)
    self:SetUpdateFunction(function()
      if GAMESTATE:GetSongBeat() < 0.125 then
        assert(callbacks == 0, "hibernation pauses the scaled tree")
      end
    end)
  end,
  Def.ActorFrame {
    Name = "Sleeping",
    OnCommand = function(self)
      local wrappers = 0
      self:AddWrapperState():SetUpdateFunction(function(_, delta)
        wrappers = wrappers + 1
        if wrappers == 1 then
          assert(math.abs(delta - 1/120) < 0.000001, "wrapper uses the unscaled wake-up delta")
        end
      end)
      self:SetUpdateRate(2):hibernate(0.125):linear(0.25):aux(1):diffusealpha(0)
      assert(self:GetUpdateRate() == 2, "update rate getter returns the native float")
      self:SetUpdateFunction(function(actor, delta)
        assert(actor:GetVisible(), "hibernation preserves visibility")
        callbacks = callbacks + 1
        if callbacks == 1 then
          assert(math.abs(delta - 1/60) < 0.000001, "owner scales its wake-up remainder")
        end
      end)
    end,
    Def.ActorFrame {
      Name = "Child",
      OnCommand = function(self)
        local calls = 0
        self:SetUpdateRate(3):linear(0.25):diffusealpha(0)
        self:SetUpdateFunction(function(_, delta)
          calls = calls + 1
          if calls == 1 then
            assert(math.abs(delta - 1/20) < 0.000001, "nested rates scale the incoming remainder")
          end
        end)
      end,
    },
  },
}
