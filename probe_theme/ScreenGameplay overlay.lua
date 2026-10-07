local cfg = ITGMANIA_HARNESS_CONFIG
local trace = ITGMANIA_HARNESS_TRACE

local probe = Def.ActorFrame {
	Name = "ITGmaniaHarnessTraceProbe",
	InitCommand = function(self)
		self:SetUpdateFunction(function(actor)
			if GAMESTATE:GetSongBeat() < cfg.until_beat then return end
			actor:SetUpdateFunction(nil)
			trace.finish()
		end)
	end,
}

return Def.ActorFrame {
	LoadActor(cfg.base_gameplay_overlay),
	probe,
}
