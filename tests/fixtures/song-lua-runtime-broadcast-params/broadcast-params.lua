local lookups = 0
return Def.ActorFrame {
	Name = "NativeBroadcastParamsControl",
	OnCommand = function(self)
		self:sleep(1)
		local params = setmetatable({ count = 2, nested = {true, "original"}, [3] = "mixed" }, {
			__index = function() lookups = lookups + 1 end,
		})
		MESSAGEMAN:Broadcast("HarnessParamsSnapshot", params)
		MESSAGEMAN:Broadcast("HarnessParamsObserved", {
			lookups = lookups,
			count = params.count,
			text = params.nested[2],
		})
		MESSAGEMAN:Broadcast("HarnessParamsEmpty", {})
	end,
	HarnessParamsSnapshotMessageCommand = function(self, params)
		params.count = 99
		params.nested[2] = "mutated"
	end,
	Def.Actor {
		OnCommand = function(self) self:sleep(0.6):queuecommand("Done") end,
		DoneCommand = function(self)
			SCREENMAN:GetTopScreen():StartTransitioningScreen("SM_DoNextScreen")
		end,
	},
}
