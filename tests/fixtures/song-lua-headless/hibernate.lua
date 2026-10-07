local callbacks = 0
return Def.ActorFrame {
  Name = "HibernateRoot",
  OnCommand = function(self)
    local sleeping = self:GetChild("Sleeping")
    sleeping:SetUpdateFunction(function(actor, delta)
      assert(actor:GetVisible(), "hibernation must not change GetVisible")
      callbacks = callbacks + 1
      if callbacks == 1 then
        assert(math.abs(delta - 1/120) < 0.000001, "wake-up keeps only the leftover delta")
      end
    end)
    self:SetUpdateFunction(function()
      if GAMESTATE:GetSongBeat() == 0 then
        assert(sleeping:GetTweenTimeLeft() == 0.375, "queue time includes hibernation")
        assert(self:GetTweenTimeLeft() == 0.375, "ActorFrame includes child queue time")
      end
      if GAMESTATE:GetSongBeat() < 0.125 then
        assert(callbacks == 0, "hibernating callbacks must not run")
        assert(sleeping:getaux() == 0, "hibernating tweens must not advance")
        assert(sleeping:GetVisible(), "hibernation preserves own visibility")
      end
    end)
  end,
  Def.ActorFrame {
    Name = "Sleeping",
    OnCommand = function(self)
      self:hibernate(0.125):linear(0.25):aux(1):diffusealpha(0)
    end,
    Def.Quad {
      Name = "Child",
      OnCommand = function(self) self:linear(0.25):diffusealpha(0) end,
    },
  },
}
