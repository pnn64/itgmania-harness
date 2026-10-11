HARNESS_END_SCENES = (HARNESS_END_SCENES or 0) + 1
HARNESS_END_ROOTS = HARNESS_END_ROOTS or 0
HARNESS_END_CHILDREN = HARNESS_END_CHILDREN or 0
local count = HARNESS_END_SCENES
local function options(level)
	return GAMESTATE:GetPlayerState(PLAYER_1):GetPlayerOptions(level)
end

return Def.ActorFrame {
	Name = "NativeEndControl",
	OnCommand = function(self)
		self:sleep(1.6)
		MESSAGEMAN:Broadcast("HarnessEndOn", {
			count = count, roots = HARNESS_END_ROOTS, children = HARNESS_END_CHILDREN,
			dark = options("ModsLevel_Current"):Dark(),
		})
		if count == 1 then
			for _, steps in ipairs(GAMESTATE:GetCurrentSong():GetStepsByStepsType("StepsType_Dance_Single")) do
				if steps:GetDifficulty() == "Difficulty_Easy" then GAMESTATE:SetCurrentSteps(PLAYER_1, steps) end
			end
			SCREENMAN:SetNewScreen("ScreenGameplay")
		end
	end,
	EndCommand = function(self)
		HARNESS_END_ROOTS = HARNESS_END_ROOTS + 1
		local stats = STATSMAN:GetCurStageStats():GetPlayerStageStats(PLAYER_1)
		MESSAGEMAN:Broadcast("HarnessEndRoot", {
			count = count, roots = HARNESS_END_ROOTS, children = HARNESS_END_CHILDREN,
			top_present = SCREENMAN:GetTopScreen() ~= nil,
			possible = stats:GetPossibleDancePoints(),
			current_possible = stats:GetCurrentPossibleDancePoints(),
			dark = options("ModsLevel_Current"):Dark(),
		})
		if count == 1 then options("ModsLevel_Preferred"):Dark(0.6) end
	end,
	Def.Actor {
		Name = "EndChild",
		OnCommand = function(self) self:sleep(0.05):queuecommand("Later") end,
		LaterCommand = function(self)
			MESSAGEMAN:Broadcast("HarnessEndLater", { count = count })
			self:sleep(1.3):queuecommand("Done")
		end,
		DoneCommand = function(self)
			SCREENMAN:GetTopScreen():StartTransitioningScreen("SM_DoNextScreen")
		end,
		EndCommand = function(self)
			HARNESS_END_CHILDREN = HARNESS_END_CHILDREN + 1
			MESSAGEMAN:Broadcast("HarnessEndChild", {
				count = count, roots = HARNESS_END_ROOTS, children = HARNESS_END_CHILDREN,
			})
			self:queuecommand("Retired")
		end,
		RetiredCommand = function(self) MESSAGEMAN:Broadcast("HarnessEndRetired") end,
	},
}
