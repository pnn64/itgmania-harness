local cfg = ITGMANIA_HARNESS_CONFIG

local input_preferences = nil
local gameplay_preferences = nil

local definitions = {}
local definition_by_table = setmetatable({}, {__mode = "k"})
local wrapped_commands = setmetatable({}, {__mode = "k"})
local actor_ids = setmetatable({}, {__mode = "k"})
local actor_paths = setmetatable({}, {__mode = "k"})
local actor_records = setmetatable({}, {__mode = "k"})
local actors = {}
local events = {}
local update_frames = {}
local random_reseeds = {}
local dropped_random_reseeds = 0
local active_context = nil
local next_sequence = 0
local dropped_events = 0
local finished = false
local scene_count = 0
local original_methods = {}
local method_hooks = setmetatable({}, {__mode = "k"})
local tween_owner_observed = setmetatable({}, {__mode = "k"})
local hooked_classes = {}
local raw_metatable = debug.getmetatable
local native_wrap = ITGMANIA_HARNESS_WRAP

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
		and (source:sub(1, 6) == "song:/"
			or source:gsub("\\", "/"):find("Songs/Harness/Probe/", 1, true) ~= nil)
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
	if type(value) ~= "table" then return false end
	if actor_ids[value] ~= nil then return true end
	-- LuaBinding::Register exposes the native inheritance table through the
	-- actual metatable. GetName/GetParent lookups on arbitrary parameter data
	-- can run authored __index functions and change the behavior we observe.
	local metatable = raw_metatable(value)
	local hierarchy = type(metatable) == "table" and rawget(metatable, "heirarchy")
	return type(hierarchy) == "table" and rawget(hierarchy, "Actor") ~= nil
end

local function native_object(value)
	local metatable = raw_metatable(value)
	local hierarchy = type(metatable) == "table" and rawget(metatable, "heirarchy")
	if type(hierarchy) ~= "table" then return nil end
	local class = nil
	for name, level in pairs(hierarchy) do if level == 0 then class = name end end
	if not class then return nil end
	local out = {type = class, path = actor_paths[value]}
	-- Invoke the saved C bindings, never an authored __index or a traced wrapper.
	if class == "Song" then
		out.song_dir = normalize_path(original_methods.Song.GetSongDir(value))
	elseif class == "Steps" then
		local song = original_methods.GameState.GetCurrentSong(GAMESTATE)
		out.song_dir = song and normalize_path(original_methods.Song.GetSongDir(song)) or nil
		for index, steps in ipairs(song and original_methods.Song.GetAllSteps(song) or {}) do
			if steps == value then out.index = index; break end
		end
		for key, method in pairs({steps_type = "GetStepsType", difficulty = "GetDifficulty",
			description = "GetDescription", meter = "GetMeter"}) do
			out[key] = original_methods.Steps[method](value)
		end
	elseif class == "Style" then
		out.name = original_methods.Style.GetName(value)
	end
	return out
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
	if kind == "userdata" then return native_object(value) or {type = kind} end
	if kind ~= "table" then return {type = kind, value = normalize_path(value)} end
	if looks_like_actor(value) then
		local id = external_actor(value)
		return {actor = id, path = actor_paths[value], name = actor_name(value)}
	end
	local object = native_object(value)
	if object then return object end
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
		out[#out + 1] = args[index] == nil and {type = "nil"} or safe_value(args[index], 0, {})
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
		scene = scene_count,
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
	if not result[1] then
		emit("error", nil, "callback.error", {}, {message = tostring(result[2])})
	end
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
			-- A returned Def constructor can lose its caller to Lua's tail-call
			-- optimization. Command functions retain their native source metadata.
			if not is_song_definition then
				for name, value in pairs(result) do
					if type(name) == "string" and name:match("Command$")
						and type(value) == "function" and song_source(function_source(value).source) then
						is_song_definition = true
						break
					end
				end
			end
			if not is_song_definition then
				for index = 1, #result do
					local child = result[index]
					if type(child) == "table" and (
						definition_by_table[child] or song_source(child._Source)
					) then
						is_song_definition = true
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
		or name:match("^[Cc]an") ~= nil
		or name:match("^[Ss]upports") ~= nil then
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
	-- StageStats and PlayerStageStats use LUA_REGISTER_CLASS userdata;
	-- actors use the instanced table binding. Retain identities for both.
	local object_result = type(result) == "table" or type(result) == "userdata"
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
		-- Numeric and enum arguments address the same native PlayerState.
		-- Identify the returned object through its original binding, without
		-- recursively adding an instrumentation query to the authored stream.
		actor_paths[result] = "player-state:" .. tostring(original_methods.PlayerState.GetPlayerNumber(result))
	elseif class == "PlayerState" and method == "GetPlayerOptions" then
		actor_paths[result] = (actor_paths[args[1]] or "player-state")
			.. "/options:" .. tostring(args[2])
	elseif object_result and class == "StatsManager" and method == "GetCurStageStats" then
		actor_paths[result] = "stage:current"
	elseif object_result and class == "StatsManager" and method == "GetPlayedStageStats" then
		actor_paths[result] = "stage:played:" .. tostring(args[2])
	elseif object_result and class == "StageStats" and method == "GetPlayerStageStats" then
		actor_paths[result] = (actor_paths[args[1]] or "stage") .. "/player:" .. tostring(args[2])
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

local function native_tween_children(actor)
	-- Observe native descendants without invoking wrapped methods or
	-- changing an actor. ActorFrame's total includes descendant tween queues.
	local children_getter = original_methods.ActorFrame and original_methods.ActorFrame.GetChildren
	local name_getter = original_methods.Actor and original_methods.Actor.GetName
	if type(children_getter) ~= "function" or type(name_getter) ~= "function" then return nil end
	local previous = active_context
	active_context = nil
	local count, seen = 0, {}
	local function visit(value, depth)
		if seen[value] then return nil end
		seen[value] = true
		if not looks_like_actor(value) then
			local nodes = {}
			if type(value) == "table" then
				for _, child in ipairs(value) do
					if count >= 64 then break end
					local node = visit(child, depth)
					if node then nodes[#nodes + 1] = node end
				end
			end
			return #nodes > 0 and {children = nodes} or nil
		end
		local left = native_tween_time_left(value)
		if type(left) ~= "number" or left <= 0 then return nil end
		local ok, name = pcall(name_getter, value)
		local node = {name = ok and name or "", time_left = left, children = {}}
		count = count + 1
		if depth >= 6 or count >= 64 then node.truncated = true; return node end
		local hierarchy = rawget(raw_metatable(value), "heirarchy")
		if rawget(hierarchy, "ActorFrame") ~= nil then
			local got, children = pcall(children_getter, value)
			if got and type(children) == "table" then
				for _, child in pairs(children) do
					if count >= 64 then node.truncated = true; break end
					local child_node = visit(child, depth + 1)
					if child_node then node.children[#node.children + 1] = child_node end
				end
			end
		end
		return node
	end
	local ok, result = pcall(visit, actor, 0)
	active_context = previous
	return ok and result or {error = tostring(result)}
end

local function install_class_hook(class)
	local methods = rawget(_G, class)
	if type(methods) ~= "table" then return end
	hooked_classes[#hooked_classes + 1] = class
	original_methods[class] = original_methods[class] or {}
	local names = {}
	for name, method in pairs(methods) do
		if type(name) == "string" and type(method) == "function" then names[#names + 1] = name end
	end
	table.sort(names)
	for _, name in ipairs(names) do
		local original = methods[name]
		local shared = method_hooks[original]
		original_methods[class][name] = shared and shared.original or original
		if shared then
			-- Reuse hooks for shared binding functions. Luna inheritance itself
			-- remains through the native class method-table metatables.
			methods[name] = shared.wrapper
		else
		methods[name] = function(...)
			local args = pack(...)
			local caller = {}
			local caller_tail = false
			for level = 2, 32 do
				local frame = debug.getinfo(level, "S")
				if not frame then break end
				if frame.what == "tail" then caller_tail = true end
				if frame.what == "Lua" or frame.what == "main" then
					caller = frame
					break
				end
			end
			local is_query = query_method(class, name, args.n)
			local should_emit = active_context ~= nil or song_source(caller.source)
			local event_args = should_emit and safe_args(args, 2) or nil
			local actor_id = nil
			if should_emit and looks_like_actor(args[1]) then actor_id = external_actor(args[1]) end
			if (name == "SetUpdateFunction" or name == "SetDrawFunction") and type(args[2]) == "function" then
				args[2] = wrap_callback(name, args[2])
			end
			local event = nil
			if should_emit then
				event = emit(is_query and "query" or event_kind(class, name), actor_id or actor_paths[args[1]],
					class .. "." .. name, event_args)
				if event then
					-- The active song command can call native methods that run theme
					-- Lua synchronously. Preserve its context and the immediate caller
					-- separately; those nested theme calls are not authored song calls.
					event.caller = {
						source = normalize_path(caller.source or ""),
						line = caller.linedefined or -1,
							tail_call = caller_tail,
					}
					event.phase = active_context and "callback" or "construction"
					if not active_context then
						event.source, event.line = event.caller.source, event.caller.line
					end
				end
			end
			local result = pack(original(unpack(args, 1, args.n)))
			assign_result_path(class, name, args, result[1])
			if event and (is_query or event.kind == "modifier") then
				event.detail = {results = safe_args(result, 1)}
				if class == "ScreenManager" and name == "GetTopScreen" and result[1] ~= nil then
					event.detail.results = {{type = "Screen", name = actor_name(result[1])}}
				end
			end
			if event and actor_id then
				local time_left = native_tween_time_left(args[1])
				if time_left ~= nil then
					event.detail = event.detail or {}
					event.detail.native_tween_time_left = time_left
					if type(time_left) == "number" and time_left > 0
						and tween_owner_observed[args[1]] ~= #update_frames
						and actor_name(args[1]):match("^PlayerP[12]$") then
						event.detail.native_tween_children = native_tween_children(args[1])
						tween_owner_observed[args[1]] = #update_frames
					end
				end
			end
			return unpack(result, 1, result.n)
		end
		if type(native_wrap) == "function" then methods[name] = native_wrap(methods[name]) end
		local hook = {original = original, wrapper = methods[name]}
		method_hooks[original], method_hooks[methods[name]] = hook, hook
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
local stage_samples = {}
local dropped_stage_samples = 0
local dropped_update_frames = 0
local stages_before = nil
local last_screen = nil

-- RandomNumbers.cpp gives Lua its own MT19937, initially seeded from wall
-- time. Set a declared input once before gameplay loads, then retain reseeds
-- across ScreenGameplay reloads instead of restarting the song's stream.
function trace.seed_random()
	local seed = MersenneTwister.Seed
	seed(cfg.random_seed)
	local function reseed(value, ...)
		seed(value, ...)
		if #random_reseeds >= cfg.max_events then
			dropped_random_reseeds = dropped_random_reseeds + 1
		else
			random_reseeds[#random_reseeds + 1] = tonumber(value) or false
		end
	end
	MersenneTwister.Seed = reseed
	math.randomseed = reseed
end

-- Capture after theme initialization, before gameplay loads any song Lua.
-- Keep these input settings separate from observed scores and life.
function trace.capture_inputs(game_mode_initialized)
	input_preferences = {
		global_offset_seconds = PREFSMAN:GetPreference("GlobalOffsetSeconds"),
		default_sync_offset = tostring(PREFSMAN:GetPreference("DefaultSyncOffset")),
		visual_delay_seconds = PREFSMAN:GetPreference("VisualDelaySeconds"),
		display_aspect_ratio = PREFSMAN:GetPreference("DisplayAspectRatio"),
	}
	local windows, players = {}, {}
	for _, window in ipairs({"W1", "W2", "W3", "W4", "W5", "Hold", "Mine", "Roll", "Attack", "Checkpoint"}) do
		windows[window] = PREFSMAN:GetPreference("TimingWindowSeconds" .. window)
	end
	for _, player in ipairs(GAMESTATE:GetEnabledPlayers()) do
		local options = GAMESTATE:GetPlayerState(player):GetPlayerOptions("ModsLevel_Preferred")
		local disabled = {}
		for _, window in ipairs(options:GetDisabledTimingWindows()) do
			disabled[#disabled + 1] = tostring(window)
		end
		players[#players + 1] = {
			player = tostring(player), fail_setting = tostring(options:FailSetting()),
			options_string = GAMESTATE:GetPlayerState(player):GetPlayerOptionsString("ModsLevel_Preferred"),
			min_tns_to_hide_notes = tostring(options:MinTNSToHideNotes()),
			disabled_timing_windows = disabled,
		}
	end
	gameplay_preferences = {
		initialization = game_mode_initialized and "theme-game-mode" or "native-preferences",
		game_mode = SL and SL.Global and SL.Global.GameMode or nil,
		mod_level = "ModsLevel_Preferred", timing_windows = windows, players = players,
		default_modifiers = PREFSMAN:GetPreference("DefaultModifiers"),
		timing_window_scale = PREFSMAN:GetPreference("TimingWindowScale"),
		timing_window_add = PREFSMAN:GetPreference("TimingWindowAdd"),
		max_input_latency_seconds = PREFSMAN:GetPreference("MaxInputLatencySeconds"),
		music_rate = GAMESTATE:GetSongOptionsObject("ModsLevel_Preferred"):MusicRate(),
	}
end

-- Clock inputs are recorded separately from stage-statistic observations.
-- Replayers use delta/music/wall time and derive their own beats and scores.
function trace.sample_update(delta)
	if finished then return end
	if #update_frames >= 1000000 then
		dropped_update_frames = dropped_update_frames + 1
		return
	end
	local beat, seconds = position()
	update_frames[#update_frames + 1] = {
		sequence = #update_frames + 1, scene = scene_count,
		delta = delta, seconds = seconds, beat = beat,
		wall_seconds = GetTimeSinceStart(),
	}
end

function trace.sample_stage(phase, stage)
	if finished then return end
	if #stage_samples >= 10000 then
		dropped_stage_samples = dropped_stage_samples + 1
		return
	end
	stage = stage or STATSMAN:GetCurStageStats()
	local beat, seconds = position()
	local gameplay_seconds = stage:GetGameplaySeconds()
	local players = {}
	for _, player in ipairs(GAMESTATE:GetEnabledPlayers()) do
		local stats = stage:GetPlayerStageStats(player)
		local steps = GAMESTATE:GetCurrentSteps(player)
		-- GetLifeRecord scales over samples-1. Two samples avoid its native
		-- division by zero and expose the initial and latest recorded life.
		local life = stats:GetLifeRecord(gameplay_seconds, 2)
		players[#players + 1] = {
			player = tostring(player),
			difficulty = steps and tostring(steps:GetDifficulty()) or nil,
			description = steps and steps:GetDescription() or nil,
			meter = steps and steps:GetMeter() or nil,
			autoplay = GAMESTATE:GetPlayerState(player):GetPlayerOptions("ModsLevel_Current"):PlayerAutoPlay(),
			possible = stats:GetPossibleDancePoints(),
			current_possible = stats:GetCurrentPossibleDancePoints(),
			actual = stats:GetActualDancePoints(),
			score = stats:GetScore(),
			current_max_score = stats:GetCurMaxScore(),
			failed = stats:GetFailed(),
			alive_seconds = stats:GetAliveSeconds(),
			life_begin = life[1],
			life_latest = life[2],
			w1 = stats:GetTapNoteScores("TapNoteScore_W1"),
			w2 = stats:GetTapNoteScores("TapNoteScore_W2"),
			misses = stats:GetTapNoteScores("TapNoteScore_Miss"),
		}
	end
	stage_samples[#stage_samples + 1] = {
		scene = scene_count, phase = phase, beat = beat, seconds = seconds,
		gameplay_seconds = gameplay_seconds, players = players,
	}
end

function trace.scene_init()
	scene_count = scene_count + 1
	if stages_before == nil then stages_before = STATSMAN:GetStagesPlayed() end
	trace.sample_stage("overlay-init")
end

function trace.screen_changed()
	local screen = SCREENMAN:GetTopScreen()
	local name = screen and screen:GetName() or nil
	if name == "ScreenGameplay" then
		trace.sample_stage("screen-active")
	elseif last_screen == "ScreenGameplay" and cfg.until_gameplay_end then
		local stages_after = STATSMAN:GetStagesPlayed()
		local committed = stages_after > stages_before
		trace.sample_stage("gameplay-exit", committed and STATSMAN:GetPlayedStageStats(1) or nil)
		trace.finish({
			reason = "gameplay-exit", next_screen = name,
			stage_committed = committed,
			stages_played_before = stages_before, stages_played_after = stages_after,
		})
	end
	last_screen = name
end

function trace.finish(completion)
	if finished then return end
	finished = true
	local roots, live_definitions = reachable_definitions()
	local song = GAMESTATE:GetCurrentSong()
	local steps = GAMESTATE:GetCurrentSteps(PLAYER_1)
	local beat, seconds = position()
	local weights = {}
	for _, name in ipairs({"W1", "W2", "W3", "W4", "W5", "Miss", "Held", "LetGo"}) do
		weights[name] = THEME:GetMetric("ScoreKeeperNormal", "PercentScoreWeight" .. name)
	end
	write_result({
		schema_version = cfg.schema_version,
		oracle = "itgmania_song_lua_semantic_trace",
		harness_version = cfg.harness_version,
		itgmania_version = ProductVersion(),
		simfile = song and normalize_path(song:GetSongFilePath()) or nil,
		title = song and song:GetDisplayFullTitle() or nil,
		theme = cfg.fallback_theme,
		game_mode = SL and SL.Global and SL.Global.GameMode or nil,
		scoring_weights = weights,
		game = GAMESTATE:GetCurrentGame():GetName(),
		style = GAMESTATE:GetCurrentStyle():GetName(),
		steps_type = steps and tostring(steps:GetStepsType()) or nil,
		difficulty = steps and tostring(steps:GetDifficulty()) or nil,
		description = steps and steps:GetDescription() or nil,
		display = {
			video_renderers = PREFSMAN:GetPreference("VideoRenderers"),
			width = DISPLAY:GetDisplayWidth(),
			height = DISPLAY:GetDisplayHeight(),
			logical_width = SCREEN_WIDTH,
			logical_height = SCREEN_HEIGHT,
		},
		trace_until_beat = cfg.until_beat,
		capture_limits = {max_events = cfg.max_events},
		random_seed = cfg.random_seed,
		random_reseeds = random_reseeds,
		dropped_random_reseeds = dropped_random_reseeds,
		completion = completion,
		input = {autoplay = cfg.autoplay, injected_inputs = false,
			requested_difficulty = cfg.difficulty, style = cfg.style,
			preferences = input_preferences, gameplay_preferences = gameplay_preferences},
		end_position = {beat = beat, seconds = seconds},
		capabilities = {
			native_random_seed = true,
			actor_definitions = true,
			child_layer_order = true,
			command_execution = true,
			actor_method_calls = true,
			callback_calls = true,
			message_broadcasts = true,
			modifier_mutations = true,
			public_tween_time = true,
			player_tween_children = true,
			player_tween_children_per_frame = true,
			draw_order_calls = true,
			native_screen_lifecycle = true,
			native_stage_statistics = true,
			native_update_frames = true,
			native_event_scenes = true,
			native_call_sources = true,
			native_constructor_queries = true,
			native_modifier_results = true,
			native_player_number_paths = true,
			native_c_method_hooks = type(native_wrap) == "function",
			native_query_classes = hooked_classes,
			native_machine_preferences = input_preferences ~= nil,
			native_display_preference = input_preferences ~= nil,
			native_initial_player_options = gameplay_preferences ~= nil,
			native_gameplay_preferences = gameplay_preferences ~= nil,
		},
		roots = roots,
		actor_definitions = live_definitions,
		runtime_actors = actors,
		events = events,
		dropped_events = dropped_events,
		stage_samples = stage_samples,
		dropped_stage_samples = dropped_stage_samples,
		update_frames = update_frames,
		dropped_update_frames = dropped_update_frames,
		update_frame_phase = "gameplay-overlay-probe",
	})
end

install_definition_hook()
if type(native_wrap) == "function" then
	local marker = {}
	local passthrough = native_wrap(function(a, b) return a, nil, b end)
	local results = pack(passthrough(marker, 7))
	assert(results.n == 3 and results[1] == marker and results[2] == nil and results[3] == 7,
		"native trace adapter changed arguments or return values")
	local ok, failure = pcall(native_wrap(function() error(marker) end))
	assert(not ok and failure == marker, "native trace adapter changed the Lua error object")
	local caller_line
	local observed = native_wrap(function()
		for level = 2, 32 do
			local frame = debug.getinfo(level, "S")
			if not frame then break end
			if frame.what == "Lua" then caller_line = frame.linedefined; break end
		end
	end)
	local function tail_helper() return observed() end
	tail_helper()
	assert(caller_line == debug.getinfo(tail_helper, "S").linedefined,
		"native trace adapter lost the authored tail-call caller")
end
local shared_player_child = Player.GetChild == ActorFrame.GetChild
for _, class in ipairs({
	"Actor", "ActorFrame", "Player", "ActorFrameTexture", "ActorMultiVertex", "ActorProxy",
	"ActorScroller", "ActorSound", "BitmapText", "Model", "Sprite", "Screen",
	"ScreenWithMenuElements", "ScreenGameplay", "StatsManager", "StageStats", "PlayerStageStats",
	"GameState", "MessageManager", "PlayerOptions", "PlayerState", "ScreenManager",
	"SongOptions", "Song", "Steps", "RageDisplay", "PrefsManager", "Style",
}) do
	install_class_hook(class)
end
if shared_player_child then
	assert(Player.GetChild == ActorFrame.GetChild, "trace hooks changed native Player:GetChild identity")
end

ITGMANIA_HARNESS_TRACE = trace
