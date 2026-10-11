local previous = -1
return Def.ActorFrame {
    OnCommand = function(self)
        self:sleep(999):SetUpdateFunction(function()
            local stats = STATSMAN:GetCurStageStats():GetPlayerStageStats(PLAYER_1)
            local points = stats:GetCurrentPossibleDancePoints()
            if points ~= previous then
                previous = points
                MESSAGEMAN:Broadcast("HarnessHoldRow", {current_possible = points})
            end
        end)
    end,
}
