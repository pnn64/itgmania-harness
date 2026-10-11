local function stats()
	return STATSMAN:GetCurStageStats():GetPlayerStageStats(PLAYER_1)
end
local function report(index)
	local pss = stats()
	MESSAGEMAN:Broadcast("HarnessPercent", {
		index = index, actual = pss:GetActualDancePoints(),
		possible = pss:GetPossibleDancePoints(), percent = pss:GetPercentDancePoints(),
	})
end
return Def.ActorFrame {
	Name = "NativePercentControl",
	OnCommand = function(self)
		local pss = stats()
		local actual, possible = pss:GetActualDancePoints(), pss:GetPossibleDancePoints()
		for index, points in ipairs({
			{0, 7}, {1, 3}, {2, 3}, {1, 7}, {123456, 1000000},
			{999949, 1000000}, {999999, 1000000}, {1000000, 1000000},
		}) do
			pss:SetDancePointLimits(points[1], points[2])
			report(index)
		end
		pss:SetDancePointLimits(actual, possible)
		self:sleep(0.5):queuecommand("Negative")
	end,
	NegativeCommand = function(self)
		report(9)
		SCREENMAN:GetTopScreen():StartTransitioningScreen("SM_DoNextScreen")
	end,
	EndCommand = function(self) report(10) end,
}
