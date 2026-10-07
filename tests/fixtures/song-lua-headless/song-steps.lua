local song = GAMESTATE:GetCurrentSong()
assert(NoteField.get_column_actors and not NoteField.set_skin)
assert(Player.SetLife and not Player.SetNoteData and not Player.SetNoteDataFromLua)
assert(Player.get_oitg_zoom_mode and Player.set_oitg_zoom_mode and not Player.oitg_zoom_mode)
local steps = song:GetAllSteps()
assert(#steps == 4)
for index, description in ipairs({"first", "second", "selected", "fourth"}) do
    assert(steps[index]:GetDescription() == description)
end
assert(steps[1]:GetDifficulty() == "Difficulty_Easy")
assert(steps[2]:GetStepsType() == "StepsType_Dance_Double")
assert(steps[3]:GetMeter() == 7)
assert(steps[3] == GAMESTATE:GetCurrentSteps(PLAYER_1))
assert(steps[3] == GAMESTATE:GetCurrentSteps(PLAYER_2))
steps[3] = nil
assert(song:GetAllSteps()[3] == GAMESTATE:GetCurrentSteps(PLAYER_1))
local singles = song:GetStepsByStepsType("StepsType_Dance_Single")
assert(#singles == 3 and singles[2] == GAMESTATE:GetCurrentSteps(PLAYER_1))
local doubles = song:GetStepsByStepsType("StepsType_Dance_Double")
assert(#doubles == 1 and doubles[1]:GetDescription() == "second")
assert(#song:GetStepsByStepsType("StepsType_Dance_Couple") == 0)
assert(not pcall(function() song:GetStepsByStepsType("not-a-steps-type") end))
return Def.ActorFrame {}
