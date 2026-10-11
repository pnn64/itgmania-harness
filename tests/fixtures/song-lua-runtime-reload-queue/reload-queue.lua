HARNESS_RELOAD_QUEUE_COUNT = (HARNESS_RELOAD_QUEUE_COUNT or 0) + 1
HARNESS_RELOAD_OLD_UPDATES = HARNESS_RELOAD_OLD_UPDATES or 0
local count = HARNESS_RELOAD_QUEUE_COUNT
if count == 1 then
	for _, steps in ipairs(GAMESTATE:GetCurrentSong():GetStepsByStepsType("StepsType_Dance_Single")) do
		if steps:GetDifficulty() == "Difficulty_Easy" then
			GAMESTATE:SetCurrentSteps(PLAYER_1, steps)
		end
	end
end

return Def.ActorFrame {
	Name = "NativeReloadQueueControl",
	InitCommand = function(self)
		-- Only the retired scene has an update function. Installing one on the
		-- replacement would conceal a stale cached actor update list.
		if count == 1 then
			self:SetUpdateFunction(function()
				HARNESS_RELOAD_OLD_UPDATES = HARNESS_RELOAD_OLD_UPDATES + 1
			end)
		end
	end,
	OnCommand = function(self)
		-- Foreground snapshots its lifetime when On runs. Keep the root alive
		-- through the child's second queue, which is added later by Tick.
		self:sleep(1.4)
		MESSAGEMAN:Broadcast("HarnessReloadQueueOn", { count = count })
		if count == 1 then SCREENMAN:SetNewScreen("ScreenGameplay") end
	end,
	Def.Actor {
		Name = "QueuedChild",
		OnCommand = function(self) self:sleep(0.05):queuecommand("Tick") end,
		TickCommand = function(self)
			MESSAGEMAN:Broadcast("HarnessReloadQueueTick", {
				count = count,
				old_updates = HARNESS_RELOAD_OLD_UPDATES,
			})
			self:sleep(1.3):queuecommand("Done")
		end,
		DoneCommand = function(self)
			MESSAGEMAN:Broadcast("HarnessReloadQueueDone", {
				count = count,
				old_updates = HARNESS_RELOAD_OLD_UPDATES,
			})
			SCREENMAN:GetTopScreen():StartTransitioningScreen("SM_DoNextScreen")
		end,
	},
}
