local label, duration, finish = ...
local updates = 0
return Def.ActorFrame {
	Name = label,
	InitCommand = function()
		MESSAGEMAN:Broadcast("HarnessForegroundConstruct", {label = label})
	end,
	OnCommand = function(self)
		self:sleep(999)
		MESSAGEMAN:Broadcast("HarnessForegroundOn", {label = label})
		self:SetUpdateFunction(function(self, delta)
			updates = updates + 1
			if updates <= 2 then
				MESSAGEMAN:Broadcast("HarnessForegroundUpdate", {label = label, delta = delta, count = updates})
			end
		end)
	end,
	Def.Actor {
		OnCommand = function(self) self:sleep(duration):queuecommand("Later") end,
		LaterCommand = function()
			MESSAGEMAN:Broadcast("HarnessForegroundLater", {label = label})
			if finish then SCREENMAN:GetTopScreen():StartTransitioningScreen("SM_DoNextScreen") end
		end,
	},
}
