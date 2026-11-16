local cfg = ITGMANIA_HARNESS_CONFIG

local function write_error(message)
	local file = RageFileUtil:CreateRageFile()
	if file:Open(cfg.result_path, 2) then
		file:Write(JsonEncode({
			schema_version = cfg.schema_version,
			oracle = "itgmania_screen_gameplay",
			error = tostring(message),
		}))
		file:Close()
	end
	file:destroy()
end

local function normalized_enum(value)
	return tostring(value):gsub("^Difficulty_", ""):lower()
end

local function configure_song()
	local song = SONGMAN:FindSong(cfg.song)
	if not song then return nil, "could not find staged song " .. cfg.song end
	if GAMESTATE:GetCurrentGame():GetName() ~= cfg.game then
		return nil, "ITGmania rejected game " .. tostring(cfg.game)
	end

	for _, player in ipairs(GAMESTATE:GetHumanPlayers()) do
		GAMESTATE:UnjoinPlayer(player)
	end
	GAMESTATE:JoinPlayer(PLAYER_1)
	if cfg.style == "versus" or cfg.style == "couple" or cfg.style == "routine" then
		GAMESTATE:JoinPlayer(PLAYER_2)
	end
	GAMESTATE:SetCurrentPlayMode("PlayMode_Regular")
	GAMESTATE:SetCurrentSong(song)
	GAMESTATE:SetCurrentStyle(cfg.style)
	if not GAMESTATE:GetCurrentStyle() or GAMESTATE:GetCurrentStyle():GetName() ~= cfg.style then
		return nil, "ITGmania rejected style " .. cfg.style
	end

	local wanted_type = tostring(GAMESTATE:GetCurrentStyle():GetStepsType())
	local selected = nil
	local all_steps = song:GetAllSteps()
	if cfg.chart_index and not all_steps[cfg.chart_index] then
		return nil, "chart index " .. cfg.chart_index .. " is outside Song:GetAllSteps (" .. #all_steps .. " charts)"
	end
	for index, steps in ipairs(all_steps) do
		local type_matches = tostring(steps:GetStepsType()) == wanted_type
		local difficulty_matches = cfg.difficulty == nil
			or normalized_enum(steps:GetDifficulty()) == cfg.difficulty:lower()
		if type_matches and difficulty_matches and (cfg.chart_index == nil or index == cfg.chart_index) then
			selected = steps
			break
		end
	end
	if not selected then
		return nil, "no " .. wanted_type .. " chart matched difficulty " .. tostring(cfg.difficulty)
			.. " at chart index " .. tostring(cfg.chart_index)
	end

	GAMESTATE:SetCurrentSteps(PLAYER_1, selected)
	if cfg.style == "versus" or cfg.style == "couple" or cfg.style == "routine" then
		GAMESTATE:SetCurrentSteps(PLAYER_2, selected)
	end
	-- Direct gameplay entry bypasses the theme's play-mode selection screen.
	-- Use its real initializer so timing windows and player fail options match
	-- normal gameplay instead of retaining the engine's stock preferences.
	ITGMANIA_HARNESS_TRACE.capture_catalog_inputs()
	local game_mode_initialized = type(SetGameModePreferences) == "function"
	if game_mode_initialized then
		SetGameModePreferences()
		THEME:ReloadMetrics()
	end
	-- A song can join the other side and reload ScreenGameplay. Configure the
	-- declared controller/profile for that side too, without joining it early.
	for _, player in ipairs({PLAYER_1, PLAYER_2}) do
		-- Profile inputs must reach the song before it constructs its splines.
		if cfg.player_options then
			for _,level in ipairs({"ModsLevel_Preferred","ModsLevel_Stage","ModsLevel_Song","ModsLevel_Current"}) do
				GAMESTATE:GetPlayerState(player):GetPlayerOptions(level):FromString(cfg.player_options)
			end
		end
		GAMESTATE:GetPlayerState(player):GetPlayerOptions("ModsLevel_Preferred"):PlayerAutoPlay(cfg.autoplay)
	end
	ITGMANIA_HARNESS_TRACE.capture_inputs(game_mode_initialized)
	ITGMANIA_HARNESS_TRACE.seed_random()
	return selected, nil
end

return Def.Actor {
	OnCommand = function()
		local steps, err = configure_song()
		if not steps then
			write_error(err)
			return
		end
		SCREENMAN:SetNewScreen("ScreenGameplay")
	end,
}
