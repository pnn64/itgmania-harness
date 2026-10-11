local cfg = ITGMANIA_HARNESS_CONFIG
local trace = ITGMANIA_HARNESS_TRACE

local probe = Def.ActorFrame {
	Name = "ITGmaniaHarnessTraceProbe",
	InitCommand = function(self)
		trace.scene_init()
		local last_second = nil
		self:SetUpdateFunction(function(actor, delta)
			trace.sample_update(delta)
			local second = math.floor(GAMESTATE:GetCurMusicSeconds())
			if second ~= last_second then
				last_second = second
				trace.sample_stage("progress")
			end
			if cfg.until_gameplay_end or GAMESTATE:GetSongBeat() < cfg.until_beat then return end
			actor:SetUpdateFunction(nil)
			trace.finish({reason = "beat-limit"})
		end)
	end,
	EndCommand = function() trace.sample_stage("screen-end") end,
}

return Def.ActorFrame {
	LoadActor(cfg.base_gameplay_overlay),
	probe,
}
