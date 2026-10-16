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
	if not GAMESTATE:GetCurrentStyle() then
		return nil, "ITGmania rejected style " .. cfg.style
	end

	local wanted_type = "StepsType_Dance_" .. (cfg.style == "versus" and "Single" or cfg.style:gsub("^%l", string.upper))
	local selected = nil
	for _, steps in ipairs(song:GetAllSteps()) do
		local type_matches = tostring(steps:GetStepsType()) == wanted_type
		local difficulty_matches = cfg.difficulty == nil
			or normalized_enum(steps:GetDifficulty()) == cfg.difficulty:lower()
		if type_matches and difficulty_matches then
			selected = steps
			break
		end
	end
	if not selected then
		return nil, "no " .. wanted_type .. " chart matched difficulty " .. tostring(cfg.difficulty)
	end

	GAMESTATE:SetCurrentSteps(PLAYER_1, selected)
	if cfg.style == "versus" or cfg.style == "couple" or cfg.style == "routine" then
		GAMESTATE:SetCurrentSteps(PLAYER_2, selected)
	end
	-- Direct gameplay entry bypasses the theme's play-mode selection screen.
	-- Use its real initializer so timing windows and player fail options match
	-- normal gameplay instead of retaining the engine's stock preferences.
	local game_mode_initialized = type(SetGameModePreferences) == "function"
	if game_mode_initialized then
		SetGameModePreferences()
		THEME:ReloadMetrics()
	end
	for _, player in ipairs(GAMESTATE:GetEnabledPlayers()) do
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
