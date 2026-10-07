local cfg = ITGMANIA_HARNESS_CONFIG

local definitions = {}
local definition_by_table = setmetatable({}, {__mode = "k"})
local wrapped_commands = setmetatable({}, {__mode = "k"})
local actor_ids = setmetatable({}, {__mode = "k"})
local actor_paths = setmetatable({}, {__mode = "k"})
local actor_records = setmetatable({}, {__mode = "k"})
local actors = {}
local events = {}
local active_context = nil
local next_sequence = 0
local dropped_events = 0
local finished = false
local original_methods = {}

local function pack(...)
	return {n = select("#", ...), ...}
end

local function normalize_path(value)
	local normalized = tostring(value):gsub("\\", "/")
	local song_path = normalized:match("Songs/Harness/Probe/(.*)")
	if song_path then return "song:/" .. song_path end
	return normalized
end

local function song_source(source)
	return type(source) == "string"
		and source:gsub("\\", "/"):find("Songs/Harness/Probe/", 1, true) ~= nil
end

local function position()
	local previous = active_context
	active_context = nil
	local beat, seconds = nil, nil
	pcall(function()
		beat = GAMESTATE:GetSongBeat()
		seconds = GAMESTATE:GetCurMusicSeconds()
	end)
	active_context = previous
	return beat, seconds
end

local function function_source(fn)
	local info = debug.getinfo(fn, "Sl") or {}
	return {
		source = normalize_path(info.source or ""),
		line = info.linedefined or 0,
		last_line = info.lastlinedefined or 0,
	}
end

local function actor_name(actor)
	local get_name = original_methods.Actor and original_methods.Actor.GetName
	if type(get_name) ~= "function" then return "" end
	local ok, name = pcall(get_name, actor)
	return ok and tostring(name or "") or ""
end

local function bind_actor(actor, definition)
	if type(actor) ~= "table" then return nil end
	if actor_ids[actor] then
		local record = actor_records[actor]
		if record and not record.definition_id then
			record.definition_id = definition.id
			record.path = definition.id
			actor_paths[actor] = definition.id
			definition.runtime_actors[#definition.runtime_actors + 1] = record.id
		end
		return actor_ids[actor]
	end
	local ordinal = #definition.runtime_actors + 1
	local id = definition.id .. (ordinal == 1 and "" or "[" .. ordinal .. "]")
	actor_ids[actor] = id
	actor_paths[actor] = definition.id
	definition.runtime_actors[#definition.runtime_actors + 1] = id
	local record = {
		id = id,
		path = definition.id,
		name = actor_name(actor),
		definition_id = definition.id,
	}
	actor_records[actor] = record
	actors[#actors + 1] = record
	return id
end

local function external_actor(actor, suggested_path)
	if type(actor) ~= "table" then return nil end
	if actor_ids[actor] then
		if suggested_path and not actor_paths[actor] then actor_paths[actor] = suggested_path end
		return actor_ids[actor]
	end
	local id = "external-" .. string.format("%04d", #actors + 1)
	local name = actor_name(actor)
	local path = suggested_path or (name ~= "" and name or id)
	actor_ids[actor] = id
	actor_paths[actor] = path
	local record = {id = id, path = path, name = name}
	actor_records[actor] = record
	actors[#actors + 1] = record
	return id
end

local function looks_like_actor(value)
	return type(value) == "table" and (
		actor_ids[value] ~= nil
		or type(value.GetName) == "function"
		or type(value.GetParent) == "function"
	)
end

local function safe_value(value, depth, seen)
	local kind = type(value)
	if kind == "nil" or kind == "boolean" or kind == "string" then
		return kind == "string" and normalize_path(value) or value
	end
	if kind == "number" then
		if value ~= value then return {type = "number", value = "nan"} end
		if value == math.huge then return {type = "number", value = "infinity"} end
		if value == -math.huge then return {type = "number", value = "-infinity"} end
		return value
	end
	if kind == "function" then
		local source = function_source(value)
		source.type = "function"
		return source
	end
	if kind ~= "table" then return {type = kind, value = normalize_path(value)} end
	if looks_like_actor(value) then
		local id = external_actor(value)
		return {actor = id, path = actor_paths[value], name = actor_name(value)}
	end
	if depth >= 5 then return {type = "table", truncated = "depth"} end
	if seen[value] then return {type = "table", cycle = true} end
	seen[value] = true

	local max_index, count, array = 0, 0, true
	for key, _ in pairs(value) do
		count = count + 1
		if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then array = false
		elseif key > max_index then max_index = key end
	end
	if array and max_index == count then
		local out = {}
		for index = 1, math.min(max_index, 256) do
			out[index] = safe_value(value[index], depth + 1, seen)
		end
		if max_index > 256 then out[257] = {truncated = max_index - 256} end
		seen[value] = nil
		return out
	end

	local keys = {}
	for key, _ in pairs(value) do keys[#keys + 1] = {key = key, name = tostring(key)} end
	table.sort(keys, function(left, right) return left.name < right.name end)
	local out = {}
	for index = 1, math.min(#keys, 256) do
		local entry = keys[index]
		out[entry.name] = safe_value(value[entry.key], depth + 1, seen)
	end
	if #keys > 256 then out.__truncated_entries = #keys - 256 end
	seen[value] = nil
	return out
end

local function safe_args(args, first)
	local out = {}
	for index = first or 1, args.n do
		out[#out + 1] = safe_value(args[index], 0, {})
	end
	return out
end

local function emit(kind, actor, operation, args, detail)
	if #events >= cfg.max_events then
		dropped_events = dropped_events + 1
		return nil
	end
	local beat, seconds = position()
	next_sequence = next_sequence + 1
	local event = {
		seq = next_sequence,
		kind = kind,
		beat = beat,
		seconds = seconds,
		actor = actor,
		operation = operation,
		args = args or {},
	}
	if active_context then
		event.definition_id = active_context.definition_id
		event.command = active_context.command
		event.callback = active_context.callback
		event.source = active_context.source
		event.line = active_context.line
	end
	if detail then event.detail = detail end
	events[#events + 1] = event
	return event
end

local function call_in_context(context, fn, ...)
	local previous = active_context
	active_context = context
	local result = pack(pcall(fn, ...))
	active_context = previous
	if not result[1] then error(result[2], 0) end
	return unpack(result, 2, result.n)
end

local function command_wrapper(definition, name, original)
	local source = function_source(original)
	local context = {
		definition_id = definition.id,
		command = name,
		source = source.source,
		line = source.line,
	}
	local wrapped = function(actor, ...)
		local actor_id = bind_actor(actor, definition)
		local previous = active_context
		active_context = context
		emit("command", actor_id, "command.begin", {}, {name = name})
		local result = pack(pcall(original, actor, ...))
		if result[1] then
			emit("command", actor_id, "command.end", {}, {name = name})
			active_context = previous
			return unpack(result, 2, result.n)
		end
		emit("error", actor_id, "command.error", {}, {
			name = name,
			message = tostring(result[2]),
		})
		active_context = previous
		error(result[2], 0)
	end
	wrapped_commands[wrapped] = {original = original, source = source}
	return wrapped, source
end

local function definition_properties(actor_def)
	local properties = {}
	for key, value in pairs(actor_def) do
		local is_command = type(key) == "string" and key:match("Command$")
		local internal = key == "Class" or key == "Name" or key == "children"
			or (type(key) == "string" and key:sub(1, 1) == "_")
		if type(key) == "string" and not is_command and not internal then
			properties[key] = safe_value(value, 0, {})
		end
	end
	return properties
end

local register_tree

local function register_definition(actor_def)
	local existing = definition_by_table[actor_def]
	if existing then return existing end
	local definition = {
		id = "def-" .. string.format("%04d", #definitions + 1),
		class = tostring(actor_def.Class or "Actor"),
		name = actor_def.Name and tostring(actor_def.Name) or nil,
		source = normalize_path(actor_def._Source or ""),
		line = tonumber(actor_def._Line) or 0,
		properties = {},
		commands = {},
		children = {},
		runtime_actors = {},
	}
	definition_by_table[actor_def] = definition
	definitions[#definitions + 1] = definition
	return definition
end

register_tree = function(actor_def, visiting)
	if type(actor_def) ~= "table" or type(actor_def.Class) ~= "string" then return nil end
	visiting = visiting or {}
	if visiting[actor_def] then return definition_by_table[actor_def] end
	visiting[actor_def] = true
	local definition = register_definition(actor_def)
	definition.class = tostring(actor_def.Class or definition.class)
	definition.name = actor_def.Name and tostring(actor_def.Name) or nil
	definition.source = normalize_path(actor_def._Source or definition.source)
	definition.line = tonumber(actor_def._Line) or definition.line
	definition.properties = definition_properties(actor_def)
	definition.children = {}

	local child_defs = {}
	for index = 1, #actor_def do child_defs[#child_defs + 1] = actor_def[index] end
	if type(actor_def.children) == "table" then
		for index = 1, #actor_def.children do
			child_defs[#child_defs + 1] = actor_def.children[index]
		end
	end
	for index, child_def in ipairs(child_defs) do
		local child = register_tree(child_def, visiting)
		if child then
			child.parent_id = definition.id
			child.layer_index = index
			definition.children[#definition.children + 1] = {
				layer_index = index,
				definition_id = child.id,
			}
		end
	end

	definition.commands = {}
	local command_names = {}
	for key, value in pairs(actor_def) do
		if type(key) == "string" and key:match("Command$") and type(value) == "function" then
			command_names[#command_names + 1] = key
		end
	end
	table.sort(command_names)
	for _, name in ipairs(command_names) do
		local current = actor_def[name]
		local prior = wrapped_commands[current]
		local original = prior and prior.original or current
		local wrapped, source = command_wrapper(definition, name, original)
		actor_def[name] = wrapped
		definition.commands[#definition.commands + 1] = {
			name = name,
			source = source.source,
			line = source.line,
			last_line = source.last_line,
		}
	end
	visiting[actor_def] = nil
	return definition
end

local function install_definition_hook()
	local metatable = getmetatable(Def)
	local original_index = metatable and metatable.__index
	if type(original_index) ~= "function" then error("Def constructor hook is unavailable") end
	metatable.__index = function(self, class)
		local constructor = original_index(self, class)
		return function(actor_def)
			-- ActorDef derives relative asset paths from the caller's stack frame.
			-- Account for this wrapper so instrumentation does not change resolution.
			local previous_level = actor_def._Level
			actor_def._Level = (previous_level or 1) + 1
			local result = constructor(actor_def)
			result._Level = previous_level
			local is_song_definition = song_source(result._Source)
			if not is_song_definition then
				for index = 1, #result do
					local child = result[index]
					if type(child) == "table" and (
						definition_by_table[child] or song_source(child._Source)
					) then
						is_song_definition = true
						if not song_source(result._Source) then result._Source = child._Source end
						break
					end
				end
			end
			if is_song_definition then register_tree(result) end
			return result
		end
	end
end

local function query_method(class, name, arg_count)
	if name:match("^[Gg]et") ~= nil
		or name:match("^[Ii]s") ~= nil
		or name:match("^[Hh]as") ~= nil
		or name:match("^[Cc]an") ~= nil then
		return true
	end
	return arg_count == 1 and (class == "PlayerOptions" or class == "SongOptions")
end

local function callback_context(method)
	if not active_context then return nil end
	return {
		definition_id = active_context.definition_id,
		command = active_context.command,
		callback = method,
		source = active_context.source,
		line = active_context.line,
	}
end

local function wrap_callback(method, callback)
	local context = callback_context(method)
	if not context or type(callback) ~= "function" then return callback end
	return function(...)
		return call_in_context(context, callback, ...)
	end
end

local function assign_result_path(class, method, args, result)
	if result == nil then return end
	if type(result) == "table" and class == "ScreenManager" and method == "GetTopScreen" then
		external_actor(result, "ScreenGameplay")
	elseif type(result) == "table" and class == "ActorFrame" and method == "GetChild" then
		local parent = actor_paths[args[1]] or actor_name(args[1])
		external_actor(result, parent .. "/" .. tostring(args[2]))
	elseif type(result) == "table" and class == "ActorFrame" and method == "GetChildren" then
		local parent = actor_paths[args[1]] or actor_name(args[1])
		for name, child in pairs(result) do
			if type(child) == "table" and #child > 0 and not looks_like_actor(child) then
				for index, duplicate in ipairs(child) do
					external_actor(duplicate, parent .. "/" .. tostring(name) .. "[" .. index .. "]")
				end
			else
				external_actor(child, parent .. "/" .. tostring(name))
			end
		end
	elseif class == "GameState" and method == "GetPlayerState" then
		actor_paths[result] = "player-state:" .. tostring(args[2])
	elseif class == "PlayerState" and method == "GetPlayerOptions" then
		actor_paths[result] = (actor_paths[args[1]] or "player-state")
			.. "/options:" .. tostring(args[2])
	end
end

local function event_kind(class, method)
	if class == "MessageManager" and method == "Broadcast" then return "message" end
	if class == "PlayerState" and method == "SetPlayerOptions" then return "modifier" end
	if class == "PlayerOptions" or class == "SongOptions" then return "modifier" end
	return "call"
end

local function native_tween_time_left(actor)
	local getter = original_methods.Actor and original_methods.Actor.GetTweenTimeLeft
	if type(getter) ~= "function" or not looks_like_actor(actor) then return nil end
	local previous = active_context
	active_context = nil
	local ok, value = pcall(getter, actor)
	active_context = previous
	if ok and type(value) == "number" then return safe_value(value, 0, {}) end
	return nil
end

local function install_class_hook(class)
	local methods = rawget(_G, class)
	if type(methods) ~= "table" then return end
	original_methods[class] = original_methods[class] or {}
	local names = {}
	for name, method in pairs(methods) do
		if type(name) == "string" and type(method) == "function" then names[#names + 1] = name end
	end
	for _, name in ipairs(names) do
		local original = methods[name]
		original_methods[class][name] = original
		methods[name] = function(...)
			local args = pack(...)
			local should_emit = active_context ~= nil and not query_method(class, name, args.n)
			local event_args = should_emit and safe_args(args, 2) or nil
			local actor_id = nil
			if should_emit and looks_like_actor(args[1]) then actor_id = external_actor(args[1]) end
			if (name == "SetUpdateFunction" or name == "SetDrawFunction") and type(args[2]) == "function" then
				args[2] = wrap_callback(name, args[2])
			end
			local event = nil
			if should_emit then
				event = emit(event_kind(class, name), actor_id or actor_paths[args[1]],
					class .. "." .. name, event_args)
			end
			local result = pack(original(unpack(args, 1, args.n)))
			if event and actor_id then
				local time_left = native_tween_time_left(args[1])
				if time_left ~= nil then
					event.detail = event.detail or {}
					event.detail.native_tween_time_left = time_left
				end
			end
			assign_result_path(class, name, args, result[1])
			return unpack(result, 1, result.n)
		end
	end
end

local function reachable_definitions()
	local reachable = {}
	local roots = {}
	local function visit(id)
		if reachable[id] then return end
		reachable[id] = true
		local number = tonumber(id:match("(%d+)$"))
		local definition = number and definitions[number] or nil
		if not definition then return end
		for _, child in ipairs(definition.children) do visit(child.definition_id) end
	end
	for _, definition in ipairs(definitions) do
		if not definition.parent_id and #definition.runtime_actors > 0 then
			roots[#roots + 1] = definition.id
			visit(definition.id)
		end
	end
	for _, definition in ipairs(definitions) do
		if #definition.runtime_actors > 0 then visit(definition.id) end
	end
	local out = {}
	for _, definition in ipairs(definitions) do
		if reachable[definition.id] then out[#out + 1] = definition end
	end
	return roots, out
end

local function write_result(document)
	local file = RageFileUtil:CreateRageFile()
	if file:Open(cfg.result_path, 2) then
		file:Write(JsonEncode(document, true))
		file:Close()
	end
	file:destroy()
end

local trace = {}

function trace.finish()
	if finished then return end
	finished = true
	local roots, live_definitions = reachable_definitions()
	local song = GAMESTATE:GetCurrentSong()
	local steps = GAMESTATE:GetCurrentSteps(PLAYER_1)
	local beat, seconds = position()
	write_result({
		schema_version = cfg.schema_version,
		oracle = "itgmania_song_lua_semantic_trace",
		harness_version = cfg.harness_version,
		itgmania_version = ProductVersion(),
		simfile = song and normalize_path(song:GetSongFilePath()) or nil,
		title = song and song:GetDisplayFullTitle() or nil,
		theme = cfg.fallback_theme,
		game = GAMESTATE:GetCurrentGame():GetName(),
		style = GAMESTATE:GetCurrentStyle():GetName(),
		steps_type = steps and tostring(steps:GetStepsType()) or nil,
		difficulty = steps and tostring(steps:GetDifficulty()) or nil,
		description = steps and steps:GetDescription() or nil,
		display = {
			width = DISPLAY:GetDisplayWidth(),
			height = DISPLAY:GetDisplayHeight(),
			logical_width = SCREEN_WIDTH,
			logical_height = SCREEN_HEIGHT,
		},
		trace_until_beat = cfg.until_beat,
		end_position = {beat = beat, seconds = seconds},
		capabilities = {
			actor_definitions = true,
			child_layer_order = true,
			command_execution = true,
			actor_method_calls = true,
			callback_calls = true,
			message_broadcasts = true,
			modifier_mutations = true,
			public_tween_time = true,
			draw_order_calls = true,
		},
		roots = roots,
		actor_definitions = live_definitions,
		runtime_actors = actors,
		events = events,
		dropped_events = dropped_events,
	})
end

install_definition_hook()
for _, class in ipairs({
	"Actor", "ActorFrame", "ActorFrameTexture", "ActorMultiVertex", "ActorProxy",
	"ActorScroller", "ActorSound", "BitmapText", "Model", "Sprite", "Screen",
	"GameState", "MessageManager", "PlayerOptions", "PlayerState", "ScreenManager",
	"SongOptions",
}) do
	install_class_hook(class)
end

ITGMANIA_HARNESS_TRACE = trace
