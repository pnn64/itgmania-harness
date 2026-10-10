-- Headless song-Lua host for the native reference harness.
-- The chunk is parsed and executed by ITGmania's bundled Lua 5.1 runtime.

local harness = assert(_HARNESS, "missing native harness context")
-- A song may replace global iterators/library tables for its own environment.
-- Keep the host's traversal and serialization independent of those changes.
setfenv(1, setmetatable({ pairs=pairs, ipairs=ipairs, type=type, next=next, unpack=unpack, table=table, string=string, math=math }, {__index=_G, __newindex=_G}))
-- Whole-chart traces retain many tables. Keep frame garbage below the default
-- two-times-live-heap threshold, and release it before the final JSON copy.
collectgarbage("setpause", 110)
-- _fallback/Scripts/00 init.lua replaces Lua's libc PRNG with these native
-- functions. Seed each capture independently, without changing song sources.
math.random = assert(MersenneTwister and MersenneTwister.Random)
math.randomseed = MersenneTwister.Seed
local random_seed = assert(_ITG_SONG_RANDOM_SEED)
math.randomseed(random_seed)
_ITG_RANDOM_RESEEDS = {}
math.randomseed = function(seed)
    _ITG_RANDOM_RESEEDS[#_ITG_RANDOM_RESEEDS + 1] = seed
    return MersenneTwister.Seed(seed)
end
-- GameManager.cpp: dance-double uses one player and eight 64-pixel columns.
local is_double = harness.steps_type == "dance-double"
local style_name = is_double and "double" or "single"
local column_count = is_double and 8 or 4
local texture_path, image_size, image_texture
local named_aft_textures = {}
local definitions, runtime_actors, external_actors, roots, events, runtime_errors = {}, {}, {}, {}, {}, {}
local callback_operation_tracks, callback_track_indexes = {}, {}
local definition_by_table = setmetatable({}, { __mode = "k" })
local actor_by_id = {}
local active_context, current_dir = nil, harness.song_dir
local current_beat, current_seconds = 0, 0
local current_bps, current_freeze, current_delay = harness.bpm / 60, false, false
local capture_operations = true
local sequence, emitted_events, dropped_events = 0, 0, 0
local scheduled_beats, manual = {}, { frames = {}, stack = {}, actors = {}, message_dispatches = {} }
local external_count, command_count = 0, 0
local projected_vertex_tracks, projected_track_by_actor, projected_signature_by_actor = {}, {}, {}
manual.models = {tracks={}, by_actor={}, buffers={}, buffer_lookup={}, lookup_count=0,
	lookup_bytes=0, buffer_hits=0, saturated_misses=0,
	vertex_fields={"local","world","view","clip","ndc","screen","uv","transformed_uv","color"}}
local update_frames = {}
local perspective_actors = {}
local tracked_players
local player_proxy_sets = { {}, {} }
local player_render_signatures = {}
local player_transform_signatures = {}
local player_render_tracks = {
	{ player = 1, path = "ScreenGameplay/PlayerP1", samples = {} },
	{ player = 2, path = "ScreenGameplay/PlayerP2", samples = {} },
}

local function normalize(path)
	path = tostring(path or ""):gsub("\\", "/")
	path = path:gsub("^//%?/", ""):gsub("^/%?/", "")
	return path:gsub("/+", "/")
end

local song_dir = normalize(harness.song_dir)
local reference_skin = _ITG_SONG_NOTESKIN
if reference_skin then
	_ITG_INIT_NOTESKIN(reference_skin.root, reference_skin.skin)
	for player = 0, 1 do
		local _, valid = _ITG_OPTIONS_UPDATE(player, "NoteSkin", reference_skin.skin)
		assert(valid, "native player noteskin initialization failed")
	end
end

local function source_path(path)
	path = normalize(path):gsub("^@", "")
	if path:sub(1, #song_dir) == song_dir then
		return "song:/" .. path:sub(#song_dir + 2)
	end
	return path
end

-- Keep dependencies loaded through computed paths (for example xero.require)
-- even when their actor mutations are attributed to the caller's command.
local loaded_lua_files = {}
local file_observations = { reads = {}, directories = {}, writes = {}, generated = {}, textures = {} }
local native_loadfile = loadfile
function loadfile(path, ...)
	local chunk, message = native_loadfile(path, ...)
	if chunk and type(path) == "string" and not file_observations.generated[source_path(path)] then loaded_lua_files[source_path(path)] = true end
	return chunk, message
end

local function dirname(path)
	return normalize(path):match("^(.*)/[^/]*$") or "."
end

local function absolute(path)
	path = normalize(path)
	return path:match("^%a:/") ~= nil or path:sub(1, 1) == "/"
end

local function join_path(base, path)
	if absolute(path) then return normalize(path) end
	return normalize(base .. "/" .. path)
end

-- ActorUtil::InitFileTypeLists and _fallback/02 ActorDef.lua classify these
-- files as Sound actors, including paths resolved without their extension.
local function sound_path(path)
	local extension = path:lower():match("%.([^./]+)$")
	return extension == "mp3" or extension == "oga" or extension == "ogg" or extension == "wav"
end
local texture_aliases, sound_aliases = {}, {}
for _, path in ipairs(harness.texture_files or {}) do
	path = normalize(path)
	local without_extension = path:gsub("%.[^./]+$", "")
	if sound_path(path) then
		sound_aliases[without_extension:lower()] = path
	else
		texture_aliases[without_extension:lower()] = path
		local without_hints = without_extension:gsub(" %d+x%d+$", "")
		texture_aliases[without_hints:lower()] = path
	end
end

local function is_file(path)
	local file = io.open(path, "rb")
	if not file then return false end
	file:close()
	return true
end

local function caller_dir()
	for level = 2, 10 do
		local info = debug.getinfo(level, "S") or {}
		local source = normalize(info.source or ""):gsub("^@", "")
		if source:sub(1, #song_dir) == song_dir then return dirname(source) end
	end
	return current_dir
end

local function finite(value)
	return value == value and value ~= math.huge and value ~= -math.huge
end

local UPDATE_FPS = 60
local bpm_segments = {}
for _, segment in ipairs(harness.bpm_segments or {}) do
	local beat, bpm = tonumber(segment[1]), tonumber(segment[2])
	if beat and bpm and finite(beat) and finite(bpm) and bpm > 0 then
		bpm_segments[#bpm_segments + 1] = { beat = beat, bpm = bpm }
	end
end
if #bpm_segments == 0 then bpm_segments[1] = { beat = 0, bpm = harness.bpm } end
table.sort(bpm_segments, function(left, right) return left.beat < right.beat end)
local timing_beat, timing_seconds = 0, 0
local timing_bpm = bpm_segments[1].beat <= 0 and bpm_segments[1].bpm or harness.bpm
for _, segment in ipairs(bpm_segments) do
	segment.seconds = timing_seconds + (segment.beat - timing_beat) * 60 / timing_bpm
	timing_beat, timing_seconds, timing_bpm = segment.beat, segment.seconds, segment.bpm
end

local function seconds_for_beat(beat)
	if _ITG_SONG_SECONDS then return _ITG_SONG_SECONDS(beat) end
	local segment = { beat = 0, seconds = 0, bpm = harness.bpm }
	for _, candidate in ipairs(bpm_segments) do
		if candidate.beat > beat then break end
		segment = candidate
	end
	return segment.seconds + (beat - segment.beat) * 60 / segment.bpm
end

local function beat_for_seconds(seconds)
	if _ITG_SONG_POSITION then return _ITG_SONG_POSITION(seconds) end
	local segment = { beat = 0, seconds = 0, bpm = harness.bpm }
	for _, candidate in ipairs(bpm_segments) do
		if candidate.seconds > seconds then break end
		segment = candidate
	end
	return segment.beat + (seconds - segment.seconds) * segment.bpm / 60
end

local function function_source(fn)
	local info = debug.getinfo(fn, "Sl") or {}
	return {
		type = "function",
		source = source_path(info.source or ""),
		line = info.linedefined or 0,
		last_line = info.lastlinedefined or 0,
	}
end

local function is_actor(value)
	return type(value) == "table" and rawget(value, "__actor") == true
end

local function safe_value(value, depth, seen)
	local kind = type(value)
	if kind == "nil" or kind == "boolean" or kind == "string" then return value end
	if kind == "number" then
		if finite(value) then return value end
		return { type = "number", value = value ~= value and "nan" or (value > 0 and "infinity" or "-infinity") }
	end
	if kind == "function" then return function_source(value) end
	if kind ~= "table" then return { type = kind, value = tostring(value) } end
	if is_actor(value) then
		return { actor = value.id, path = value.path, name = value.__songlua_name }
	end
	if depth >= 5 then return { type = "table", truncated = "depth" } end
	if seen[value] then return { type = "table", cycle = true } end
	seen[value] = true
	local count, max_index, array = 0, 0, true
	for key in pairs(value) do
		count = count + 1
		if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then
			array = false
		elseif key > max_index then
			max_index = key
		end
	end
	local out = {}
	if array and max_index == count then
		for index = 1, math.min(max_index, 256) do
			out[index] = safe_value(value[index], depth + 1, seen)
		end
		if max_index > 256 then out[257] = { truncated = max_index - 256 } end
	else
		local keys = {}
		for key in pairs(value) do keys[#keys + 1] = { key = key, name = tostring(key) } end
		table.sort(keys, function(left, right) return left.name < right.name end)
		for index = 1, math.min(#keys, 256) do
			local item = keys[index]
			out[item.name] = safe_value(value[item.key], depth + 1, seen)
		end
		if #keys > 256 then out.__truncated_entries = #keys - 256 end
	end
	seen[value] = nil
	return out
end

_ITG_JSON_NULL = {}
local function safe_args(...)
	local out = {}
	for index = 1, select("#", ...) do out[index] = safe_value(select(index, ...), 0, {}) or (select(index, ...) == nil and _ITG_JSON_NULL or false) end
	return out
end

local function emit(kind, actor, operation, args, detail)
	if not capture_operations and active_context
		and operation ~= "Sprite.Load"
		-- Boolean getter/return audits compare every write, including repeated
		-- values. Their sequence must not use the setter sampling cadence.
		and not (detail and (detail.boolean_option or detail.noteskin_option))
		and (active_context.command == "UpdateCommand" or active_context.callback == "SetUpdateFunction"
			or active_context.callback == "SetDrawFunction" or active_context.recurring) then
		return
	end
	if emitted_events >= harness.max_events then
		dropped_events = dropped_events + 1
		return
	end
	emitted_events = emitted_events + 1
	sequence = sequence + 1
	local event = {
		seq = sequence,
		kind = kind,
		beat = current_beat,
		seconds = current_seconds,
		actor = actor and (actor.id or actor.path) or nil,
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
	if kind == "call" and active_context and active_context.callback then
		local key = table.concat({
			tostring(event.actor or ""), operation,
			tostring(event.definition_id or ""), tostring(event.command or ""),
			tostring(event.callback or ""), tostring(event.source or ""),
			tostring(event.line or ""),
		}, "\31")
		local index = callback_track_indexes[key]
		if not index then
			index = #callback_operation_tracks + 1
			callback_track_indexes[key] = index
			callback_operation_tracks[index] = {
				actor = event.actor, operation = operation,
				definition_id = event.definition_id, command = event.command,
				callback = event.callback, source = event.source, line = event.line,
				samples = {},
			}
		end
		local samples = callback_operation_tracks[index].samples
		samples[#samples + 1] = { event.seq, event.beat, event.seconds, event.args }
		return
	end
	events[#events + 1] = event
	return event
end

local function record_runtime_error(actor, operation, name, message)
	local item = {
		beat = current_beat,
		seconds = current_seconds,
		actor = actor and (actor.id or actor.path) or nil,
		operation = operation,
		name = name,
		message = tostring(message),
	}
	if active_context then
		item.definition_id = active_context.definition_id
		item.command = active_context.command
		item.callback = active_context.callback
		item.source = active_context.source
		item.line = active_context.line
	end
	runtime_errors[#runtime_errors + 1] = item
	emit("error", actor, operation, {}, { name = name, message = tostring(message) })
end

local function json_escape(value)
	return value:gsub('[%z\1-\31\\"]', function(char)
		local escapes = { ['"'] = '\\"', ['\\'] = '\\\\', ['\b'] = '\\b', ['\f'] = '\\f', ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t' }
		return escapes[char] or string.format("\\u%04x", string.byte(char))
	end)
end

local function json_encode(value)
	local parts, bytes = {}, 0
	local function flush()
		if bytes == 0 then return end
		_ITG_JSON_APPEND(table.concat(parts))
		parts, bytes = {}, 0
	end
	local function emit(text)
		parts[#parts + 1], bytes = text, bytes + #text
		if bytes >= 65536 then flush() end
	end
	local seen = {}
	local function encode(item)
        if item == _ITG_JSON_NULL then emit("null"); return end
		local kind = type(item)
		if kind == "nil" then emit("null"); return end
		if kind == "boolean" then emit(item and "true" or "false"); return end
		if kind == "number" then emit(finite(item) and string.format("%.17g", item) or "null"); return end
		if kind == "string" then emit('"' .. json_escape(item) .. '"'); return end
		if kind ~= "table" then encode(tostring(item)); return end
		if seen[item] then error("cycle in semantic document") end
		seen[item] = true
		local metadata = getmetatable(item)
		local count, max_index, array = 0, 0, not (type(metadata) == "table" and metadata._ITG_JSON_OBJECT)
		for key in pairs(item) do
			count = count + 1
			if type(key) ~= "number" or key < 1 or key % 1 ~= 0 then array = false
			elseif key > max_index then max_index = key end
		end
		if array and max_index == count then
			emit("[")
			for index = 1, max_index do
				if index > 1 then emit(",") end
				encode(item[index])
			end
			emit("]")
		else
			local keys = {}
			for key in pairs(item) do keys[#keys + 1] = tostring(key) end
			table.sort(keys)
			emit("{")
			for index, key in ipairs(keys) do
				if index > 1 then emit(",") end
				encode(key); emit(":"); encode(item[key])
			end
			emit("}")
		end
		seen[item] = nil
	end
	encode(value)
	flush()
	return ""
end

local actor_def_mt = {}

local function merge_actor_defs(left, right)
	local out = {}
	for key, value in pairs(left or {}) do out[key] = value end
	for key, value in pairs(right or {}) do
		local first = out[key]
		if type(first) == "function" and type(value) == "function" then
			local second = value
			value = function(...)
				first(...)
				return second(...)
			end
		end
		out[key] = value
	end
	return setmetatable(out, getmetatable(left))
end

actor_def_mt.__concat = merge_actor_defs

Def = setmetatable({}, {
	__index = function(_, class)
		return function(definition)
			definition = definition or {}
			definition.Class = definition.Class or class
			local info = debug.getinfo(2, "Sl") or {}
			definition._Source = definition._Source or source_path(info.source or "")
			definition._ModelDirectory = definition._ModelDirectory or dirname(normalize(info.source or ""):gsub("^@", ""))
			definition._Line = definition._Line or info.currentline or 0
			return setmetatable(definition, actor_def_mt)
		end
	end,
})

local function resolve_actor_path(path, base)
	path = normalize(path)
	base = base or caller_dir(4)
	local exact = join_path(base, path)
	if is_file(exact) then return exact end
	for _, candidate in ipairs({ exact .. ".lua", exact .. "/default.lua", exact .. "/Default.lua" }) do
		if is_file(candidate) then return candidate end
	end
	if sound_aliases[exact:lower()] then return sound_aliases[exact:lower()] end
	return exact
end

local function execute_file(path, ...)
	local prior = current_dir
	current_dir = dirname(path)
	local chunk, load_error = loadfile(path)
	if not chunk then
		current_dir = prior
		error(load_error, 0)
	end
	local result = { pcall(chunk, ...) }
	current_dir = prior
	if not result[1] then error(result[2], 0) end
	return result[2]
end

function LoadActor(path, ...)
	local resolved = resolve_actor_path(path)
	if resolved:lower():match("%.lua$") and is_file(resolved) then return execute_file(resolved, ...) end
	if sound_path(resolved) then return Def.Sound { File = source_path(resolved) } end
	return Def.Sprite { Texture = source_path(resolved) }
end

function LoadActorWithParams(path, params) return LoadActor(path, params) end
function LoadFont(path) return Def.BitmapText { Font = tostring(path) } end
function LoadActorForNoteSkin(button, element)
	if reference_skin then return NOTESKIN:LoadActor(button, element) end
	error("native noteskin resources unavailable", 0)
end

local function color_component(hex, offset)
	return tonumber(hex:sub(offset, offset + 1), 16) / 255
end

function color(value)
	if type(value) == "table" then return value end
	value = tostring(value or "")
	local hex = value:match("^#(%x%x%x%x%x%x%x?%x?)$")
	if hex then
		if #hex == 6 then hex = hex .. "ff" end
		return { color_component(hex, 1), color_component(hex, 3), color_component(hex, 5), color_component(hex, 7) }
	end
	local out = {}
	for part in value:gmatch("[^,]+") do out[#out + 1] = tonumber(part) or 0 end
	while #out < 4 do out[#out + 1] = #out == 3 and 1 or 0 end
	return out
end

function lerp(percent, left, right) return left + (right - left) * percent end
function scale(value, low, high, out_low, out_high) return out_low + (value - low) * (out_high - out_low) / (high - low) end
function clamp(value, low, high) return math.max(low, math.min(high, value)) end
function round(value) return math.floor(value + 0.5) end
function split(separator, value)
	local out = {}
	for part in tostring(value):gmatch("([^" .. separator .. "]+)") do out[#out + 1] = part end
	return out
end
function join(separator, ...)
	local values = {...}
	if #values == 1 and type(values[1]) == "table" then values = values[1] end
	return table.concat(values, separator)
end
-- _fallback/Scripts/01 base.lua: honor indexed lookups and live table changes.
function ivalues(t)
	local n = 0
	return function()
		n = n + 1
		return t[n]
	end
end
function ipairs_o(values) return ivalues(values) end
function ToEnumShortString(value) return tostring(value):match("[^_]+$") or tostring(value) end
function ProductVersion() return harness.itgmania_version end
function GetProductVersion()
	local version = {}
	for number in ProductVersion():gsub("-.*", ""):gmatch("[^%.]+") do version[#version + 1] = tonumber(number) end
	return version
end
function IsMinimumProductVersion(...)
	local version = GetProductVersion()
	for index = 1, select("#", ...) do
		local minimum = select(index, ...)
		if not version[index] or version[index] < minimum then return false end
		if version[index] > minimum then return true end
	end
	return true
end
function GetScreenAspectRatio() return harness.screen_width / harness.screen_height end
function WideScale(narrow, wide) return scale(harness.screen_width, 640, 854, narrow, wide) end
function ProductFamily() return "StepMania" end
function ProductID() return "ITGmania" end
-- _fallback/Scripts/02 Actor.lua background sizing helpers.
bg_fit_functions = {
	BackgroundFitMode_CoverDistort = function(actor, width, height) actor:zoomx(width / actor:GetWidth()):zoomy(height / actor:GetHeight()) end,
	BackgroundFitMode_CoverPreserve = function(actor, width, height) actor:zoom(math.max(width / actor:GetWidth(), height / actor:GetHeight())) end,
	BackgroundFitMode_FitInside = function(actor, width, height) actor:zoom(math.min(width / actor:GetWidth(), height / actor:GetHeight())) end,
	BackgroundFitMode_FitInsideAvoidLetter = function(actor, width, height) actor:zoom(height / actor:GetHeight()) end,
	BackgroundFitMode_FitInsideAvoidPillar = function(actor, width, height) actor:zoom(width / actor:GetWidth()) end,
}
function Trace() end
function Warn() end
-- Simply Love (06 SL-Utilities.lua) defines SM(); songs written for that theme
-- call it. Table arguments go through its TableToString, which is not modelled.
function SM(arg, duration, stack)
	if type(arg) == "table" then error("SM: table arguments are not modelled") end
	MESSAGEMAN:Broadcast("SystemMessage", {Message = tostring(arg), Duration = duration, Stack = stack})
end
lua = {
	ReportScriptError = function(message)
        if type(message) ~= "string" and type(message) ~= "number" then error("string expected") end
        if lua.reporting then return end
        lua.reporting = true
        MESSAGEMAN:Broadcast("ScriptError", {message = tostring(message)})
        lua.reporting = false
    end,
    ReadFile = function(path)
        local file = RageFileUtil.CreateRageFile()
        local value = file:Open(normalize(path), 1) and file:Read() or ""
        file:Close(); file:destroy()
        return value
    end,
    WriteFile = function(path, text)
        local file = RageFileUtil.CreateRageFile()
        local opened = file:Open(path, 2)
        if opened then file:Write(text); file:Close() end
        file:destroy()
        return opened
    end,
}

math.mod = math.mod or math.fmod
math.atan2 = math.atan2 or function(y, x) return math.atan(y / x) end

-- Keep native prototypes inside the actor metatable: Lua 5.1 limits each
-- function to 200 locals, and the whole-song host already reaches that limit.
local actor_mt = { classes = {
    native = {
        Actor = Actor, ActorFrame = ActorFrame, ActorFrameTexture = ActorFrameTexture,
        ActorMultiVertex = ActorMultiVertex, Sprite = Sprite,
        Model = Model,
        ActorProxy = ActorProxy, BitmapText = BitmapText,
    },
    bases = {
        ActorFrame = "Actor", ActorFrameTexture = "ActorFrame",
        ActorMultiVertex = "Actor", Sprite = "Actor",
        Model = "Actor",
        ActorProxy = "Actor", BitmapText = "Actor",
    },
} }

local function external_actor(path, class)
	external_count = external_count + 1
	local actor = {
		__actor = true,
		id = "external-" .. string.format("%04d", external_count),
		path = path,
		__songlua_name = path:match("([^/]+)$") or path,
		class = class or "ActorFrame",
		children = {}, children_by_name = {}, wrappers = {},
		state = {}, tweens = {},
	}
	actor.identity = tostring(actor):match("^table: (.+)$")
	external_actors[#external_actors + 1] = {
		id = actor.id, path = actor.path, name = actor.__songlua_name, class = actor.class,
	}
	actor_by_id[actor.id] = actor
	return setmetatable(actor, actor_mt)
end

local function event_operation(actor, name)
	local class = actor.class
	if name == "SortByDrawOrder" or name == "SetDrawByZPosition" or name == "GetChild" or name == "GetChildren" then class = "ActorFrame" end
	return class .. "." .. name
end

local function state_key(name)
	local key = name:gsub("^[Ss]et", ""):gsub("^[Gg]et", "")
	return key:lower()
end

local function int_boolean(value)
	if type(value) == "number" then return value ~= 0 end
	return value == true
end

local run_command
local run_command_tree
local broadcast

local TWEEN_DEFAULTS = {
	x = 0, y = 0, z = 0,
	zoom = 1, zoomx = 1, zoomy = 1, zoomz = 1,
	rotationx = 0, rotationy = 0, rotationz = 0,
	skewx = 0, skewy = 0,
	cropleft = 0, croptop = 0, cropright = 0, cropbottom = 0,
	fadeleft = 0, fadetop = 0, faderight = 0, fadebottom = 0,
	diffuse = { 1, 1, 1, 1 },
	glow = { 1, 1, 1, 0 }, aux = 0,
}

local function copy_value(value)
	if type(value) ~= "table" then return value end
	local out = {}
	for key, item in pairs(value) do out[key] = copy_value(item) end
	return out
end

local function tween_state(state)
	local out = {}
	for key, default in pairs(TWEEN_DEFAULTS) do
		local value = state[key]
		if value == nil then value = default end
		out[key] = copy_value(value)
	end
	return out
end

local function merge_tween_state(state, source)
	for key, value in pairs(source) do state[key] = copy_value(value) end
end

-- Native macros select one effect, reset its timer only on a mode change,
-- and restore the macro's period and color defaults on every invocation.
local COLOR_EFFECTS = {
    diffuseblink = { { .5,.5,.5,.5 }, { 1,1,1,1 } },
    diffuseshift = { { 0,0,0,1 }, { 1,1,1,1 } },
    diffuseramp = { { 0,0,0,1 }, { 1,1,1,1 } },
    glowblink = { { 1,1,1,.2 }, { 1,1,1,.8 } },
    glowshift = { { 1,1,1,.2 }, { 1,1,1,.8 } },
    glowramp = { { 1,1,1,.2 }, { 1,1,1,.8 } },
}

local function dest_state(actor)
	local tweens = actor.tweens or {}
	return #tweens > 0 and tweens[#tweens].state or actor.state
end

local function state_value(state, key)
	local value = state[key]
	if value ~= nil then return value end
	return copy_value(TWEEN_DEFAULTS[key])
end

local function set_tween_value(actor, key, value)
	-- LunaActor converts scalar arguments to float before Actor stores them.
	-- Keep that rounding visible to subsequent Lua getters and branch tests.
	if type(value) ~= "table" then value = _ITG_FLOAT(value) end
	dest_state(actor)[key] = value
end

local function add_tween_value(actor, key, value)
	local state = dest_state(actor)
	-- Native AddX/AddRotation first narrow their argument, then add two floats.
	state[key] = _ITG_FLOAT(_ITG_FLOAT(state_value(state, key)) + _ITG_FLOAT(value))
end

local function tween_time_left(actor)
	local hibernation = tonumber(rawget(actor, "hibernate_seconds")) or 0
	local total = hibernation
	for _, tween in ipairs(actor.tweens or {}) do total = _ITG_FLOAT(total + tween.time_left) end
	if actor.class == "ActorFrame" or actor.class == "ActorFrameTexture" then
		for _, child in ipairs(actor.children or {}) do
			total = math.max(total, _ITG_FLOAT(hibernation + tween_time_left(child)))
		end
	end
	return total
end

local function begin_tween(actor, duration, easing, command, event)
	if event then
		-- Actor::GetTweenTimeLeft sums own hibernation and queue floats.
		-- ActorFrame's child maximum must not delay its own new tween.
		local start = tonumber(rawget(actor, "hibernate_seconds")) or 0
		for _, tween in ipairs(actor.tweens or {}) do start = _ITG_FLOAT(start + tween.time_left) end
		event.detail = event.detail or {}
		event.detail.queue_start_seconds = start
	end
	actor.tweens = actor.tweens or {}
	local source = #actor.tweens > 0 and actor.tweens[#actor.tweens].state or actor.state
	duration = _ITG_FLOAT(math.max(0, tonumber(duration) or 0))
	actor.tweens[#actor.tweens + 1] = {
		state = tween_state(source), duration = duration,
		time_left = duration, easing = easing or "linear",
		command = command,
	}
end

local function tween_percent(easing, percent)
	if type(easing) == "table" then return _ITG_BEZIER_PERCENT(percent, easing) end
	if easing == "accelerate" then return percent * percent end
	if easing == "decelerate" then return 1 - (1 - percent) * (1 - percent) end
	if easing == "smooth" then return percent * percent * (3 - 2 * percent) end
	if easing == "spring" then return 1 - math.cos(percent * math.pi * 2.5) / (1 + percent * 3) end
	return percent
end

local function tween_value(left, right, percent)
	if type(left) == "number" and type(right) == "number" then return _ITG_ACTOR_LERP(left, right, percent) end
	if type(left) == "table" and type(right) == "table" then
		local out = {}
		for key, value in pairs(right) do out[key] = tween_value(left[key] or value, value, percent) end
		return out
	end
	return percent >= 1 and copy_value(right) or copy_value(left)
end

local function apply_tween(actor, start, finish, percent)
	for key in pairs(TWEEN_DEFAULTS) do
		if start[key] ~= nil or finish[key] ~= nil then
			actor.state[key] = tween_value(state_value(start, key), state_value(finish, key), percent)
		end
	end
end

local function advance_tween(actor, delta)
	-- Actor::UpdateTweening uses float subtraction and requires positive delta
	-- before advancing even a zero-time state. Double precision changes which
	-- frame releases sleep(0), especially after a long chain of short tweens.
	local remaining = _ITG_FLOAT(math.max(0, tonumber(delta) or 0))
	while #(actor.tweens or {}) > 0 and remaining > 0 do
		local tween = actor.tweens[1]
		local beginning = tween.start == nil
		if beginning then tween.start = tween_state(actor.state) end
		local elapsed = math.min(tween.time_left, remaining)
		tween.time_left = _ITG_FLOAT(tween.time_left - elapsed)
		remaining = _ITG_FLOAT(remaining - elapsed)
		if tween.time_left == 0 then
			merge_tween_state(actor.state, tween.state)
			table.remove(actor.tweens, 1)
		else
			local percent = tween.duration == 0 and 1 or _ITG_FLOAT(1 - _ITG_FLOAT(tween.time_left / tween.duration))
			apply_tween(actor, tween.start, tween.state, tween_percent(tween.easing, percent))
		end
		if beginning and tween.command then
            if tween.command:sub(1, 1) == "!" then broadcast(tween.command:sub(2), {})
            else run_command_tree(actor, tween.command) end
        end
	end
end

local function child_result(children)
	if #children == 1 then return children[1] end
	local group = {}
	for i, child in ipairs(children) do group[i] = child end
	return setmetatable(group, { __index = function(items, key)
		if tonumber(key) then return rawget(items, key) end
		local method = items[#items][key]
		return function(self, ...) return method(self[#self], ...) end
	end })
end

local function typed_child(actor, name, class)
	if actor.children_by_name and actor.children_by_name[name] then
		local children = actor.children_by_name[name]
		return child_result(children)
	end
	actor.children_by_name = actor.children_by_name or {}
	local child = external_actor(actor.path .. "/" .. tostring(name), class or "ActorFrame")
	child.parent = actor
	if class == "Spline" and name == "GetSpline" then child.native_spline = _ITG_SPLINE_NEW() end
	actor.children_by_name[name] = { child }
	actor.children[#actor.children + 1] = child
	return child
end

local function actor_child(actor, name) return typed_child(actor, name, "ActorFrame") end

local function actor_texture(actor)
	local texture = actor.state.texture
	return type(texture) == "string" and named_aft_textures[_ITG_TEXTURE_NAME(texture)] or texture
end

local function actor_size(actor)
	local texture = actor_texture(actor)
	if type(texture) == "table" and texture.class == "RageTexture" then
		return actor.state.width or texture.state.sourceframewidth or 1,
			actor.state.height or texture.state.sourceframeheight or 1
	end
	local width, height = 1, 1
	local path = actor.class == "Sprite" and texture_path(actor)
	if path then width, height = image_size(path) end
	return actor.state.width or width or 1, actor.state.height or height or 1
end

manual.models.methods = {position=true,playanimation=true,SetDefaultAnimation=true,
    GetDefaultAnimation=true,loop=true,rate=true,GetNumStates=true}
manual.models.actor_methods = {animate=true,play=true,pause=true,setstate=true,hibernate=true,
    texturetranslate=true,texturewrapping=true,SetTextureFiltering=true,blend=true,
    zbuffer=true,ztest=true,ztestmode=true,zwrite=true,zbias=true,clearzbuffer=true,
    backfacecull=true,cullmode=true}

local function actor_call(actor, name, ...)
    if rawget(actor, "native_model") and manual.models.methods[name] then
        local result, message = _ITG_MODEL_CALL(actor.native_model, name, ...)
        if message then error(message, 0) end
        if name:match("^Get") then return result end
        emit("call", actor, event_operation(actor, name), safe_args(...))
        return actor
    end
    if rawget(actor, "native_model") and manual.models.actor_methods[name] then
        local result, message = _ITG_MODEL_CALL(actor.native_model, name, ...)
        if message then error(message, 0) end
    end
    if rawget(actor, "native_spline") then
        -- _fallback's camel aliases point at these linked LunaCubicSplineN methods.
        local native_name = name:gsub("%u", function(char) return "_" .. char:lower() end):gsub("^_", "")
        local result = {actor.native_spline[native_name](actor.native_spline, ...)}
        if native_name:match("^set_") or native_name == "solve" then
            emit("call", actor, event_operation(actor, name), safe_args(...))
            return actor
        end
        return unpack(result)
    end
	local args = safe_args(...)
	if name == "rainbowscroll" or name == "jitter" or name == "uppercase" then
		-- BArg is strict; BIArg and lua_toboolean have different contracts.
		if type((...)) ~= "boolean" then error(name .. ": boolean expected") end
	end
	if name == "LoadBackground" or name == "LoadBanner" then
		local value = (...)
		if type(value) ~= "string" and type(value) ~= "number" then error(name .. ": string expected") end
	end
	if name == "GetTarget" and actor.class == "ActorProxy" then return actor.state.target end
	if name == "SetTarget" and actor.class == "ActorProxy" then
		local target = (...)
		if type(target) ~= "table" or not rawget(target, "__actor") then
			error("ActorProxy.SetTarget requires an Actor")
		end
	end
	if name == "GetName" then return actor.__songlua_name or "" end
	if name == "GetText" then return actor.state.text or "" end
	if name == "get_mult_attrs_with_diffuse" then return actor.state.mult_attrs_with_diffuse == true end
	if name == "GetWidth" or name == "GetHeight" then
		local width, height = actor_size(actor)
		return name == "GetWidth" and width or height
	end
	if name == "GetZoomedWidth" or name == "GetZoomedHeight" then
		local width, height = actor_size(actor)
		local axis = name == "GetZoomedWidth" and "x" or "y"
		return (axis == "x" and width or height) * state_value(dest_state(actor), "zoom" .. axis)
			* (actor.state["basezoom" .. axis] or 1)
	end
	-- These are fallback-theme Lua methods, expressed in native actor primitives.
	if name == "Center" then return actor_call(actor, "xy", harness.screen_width / 2, harness.screen_height / 2) end
	if name == "CenterX" then return actor_call(actor, "x", harness.screen_width / 2) end
	if name == "CenterY" then return actor_call(actor, "y", harness.screen_height / 2) end
	if name == "align" then
		actor_call(actor, "halign", (...))
		return actor_call(actor, "valign", select(2, ...))
	end
	if name == "FullScreen" then return actor_call(actor, "stretchto", 0, 0, harness.screen_width, harness.screen_height) end
	if name == "scale_or_crop_background" or name == "scale_or_crop_background_no_move" then
		-- _fallback/Scripts/02 Actor.lua delegates to this preference's fit
		-- function, then centers only the moving variant.
		local fit = bg_fit_functions[PREFSMAN:GetPreference("BackgroundFitMode")]
		if fit then fit(actor, harness.screen_width, harness.screen_height)
		else actor_call(actor, "scaletocover", 0, 0, harness.screen_width, harness.screen_height) end
		if name == "scale_or_crop_background" then actor_call(actor, "Center") end
		return actor
	end
    if name == "getrotation" then return state_value(dest_state(actor), "rotationx"), state_value(dest_state(actor), "rotationy"), state_value(dest_state(actor), "rotationz") end
    if name == "GetParent" then return actor.parent end
    if name == "get" and actor.class == "Sound" then return typed_child(actor, "Sound", "RageSound") end
    if name == "GetPlayerInfo" and actor.class == "Screen" then
        local player = tonumber((...)) or ((...) == "PlayerNumber_P2" and 1 or 0)
        if is_double and player == 1 then return nil end
        return { GetLifeMeter = function() return actor_call(actor, "GetLifeMeter", player) end }
    end
    if name == "GetLifeMeter" and actor.class == "Screen" then
        return typed_child(actor, "LifeMeterP" .. ((tonumber((...)) or 0) + 1), "LifeMeterBar")
    end
    if name == "GetLife" and actor.class == "LifeMeterBar" then return 0.5 end
	if name == "GetChild" then
        local child_name = (...)
        -- ScreenGameplay::Init adds only enabled PlayerInfo actors. A missing
        -- double-mode PlayerP2 must stay absent, including repeated lookups.
        if actor.class == "Screen" and child_name:match("^PlayerP[12]$")
            and not actor.children_by_name[child_name] then return nil end
        -- Song ActorFrames already own their complete child list. Native
        -- ActorFrame::PushChildTable returns nil and creates no missing child.
        if (rawget(actor, "definition_id") or rawget(actor, "child_lookup_exact"))
            and not actor.children_by_name[child_name] then return nil end
        return actor_child(actor, child_name)
    end
	if name == "GetChildren" then
		-- ActorFrame::PushChildrenTable inserts in m_SubActors order. A name
		-- map traversal produces a different Lua 5.1 table and draw order.
		local out, groups = {}, {}
		for _, child in ipairs(actor.children or {}) do
			local child_name = child.__songlua_name
			if out[child_name] == nil then out[child_name] = child
			elseif groups[child_name] then
				local group = groups[child_name]
				group[#group + 1] = child
			else
				local group = child_result({out[child_name], child})
				groups[child_name], out[child_name] = group, group
			end
		end
		return out
	end
	if name == "GetNumChildren" then return #(actor.children or {}) end
	if name == "GetTextureCoordRect" and actor.class == "RageTexture" then
		local rects = actor.state.framerects
		if rects then
			local frame = tonumber((...))
			if not frame or frame < 0 then error("frame index must be nonnegative", 2) end
			-- Rectangles come from RageTexture::GetTextureCoordRect, including
			-- native image/allocation scaling, and stay with this texture proxy.
			return unpack(rects[math.floor(frame) % #rects + 1])
		end
	end
	if name == "GetUpdateRate" then return rawget(actor, "update_rate") or 1 end
	if name == "GetNumWrapperStates" then return #(rawget(actor, "wrappers") or {}) end
	if name == "GetWrapperState" then
		local index = tonumber((...)) or 1
		local wrappers = rawget(actor, "wrappers") or {}
		rawset(actor, "wrappers", wrappers)
		wrappers[index] = wrappers[index] or external_actor(actor.path .. "/WrapperState" .. index, "ActorFrame")
		return wrappers[index]
	end
	if name == "GetColumnActors" or name == "get_column_actors" then
		local out = {}
		for index = 1, column_count do out[index] = typed_child(actor, "Column" .. index, "NoteColumnRenderer") end
		return out
	end
	if name == "GetTexture" then
		if actor.class ~= "ActorFrameTexture" then
			local texture = actor_texture(actor)
			if type(texture) == "table" and texture.class == "RageTexture" then return texture end
			local path = texture_path(actor)
			return path and image_texture(path) or nil
		end
		return rawget(actor, "allocated_texture")
	end
	if name == "GetPath" and actor.class == "RageTexture" then return _ITG_TEXTURE_NAME(actor.state.path or "") end
	if name == "GetPlayerStageStats" then return external_actor(actor.path .. "/PlayerStageStats", "PlayerStageStats") end
	if name == "GetPercentDancePoints" then return 0 end
	if name == "GetSpline" or name == "get_spline" or name:match("Handler$") or name:match("^get_.*_handler$") then
		local canonical = ({get_spline="GetSpline", get_pos_handler="GetPosHandler",
			get_rot_handler="GetRotHandler", get_zoom_handler="GetZoomHandler"})[name] or name
		return typed_child(actor, canonical, "Spline")
	end
	if name == "GetTweenTimeLeft" then return tween_time_left(actor) end
	if name == "GetSecsIntoEffect" then return actor.state.spinclock or 0 end
	if name == "GetEffectDelta" then return actor.effect_delta or 0 end
	if name == "GetVisible" then return actor.state.visible ~= false end
	if name == "GetX" or name == "GetY" or name == "GetZ" then
		return state_value(actor.state, name:sub(4):lower())
	end
	if name == "GetDestX" or name == "GetDestY" or name == "GetDestZ" then
		return state_value(dest_state(actor), name:sub(8):lower())
	end
	if name == "GetDiffuseAlpha" then
		return state_value(dest_state(actor), "diffuse")[4]
	end
	if name == "GetDiffuse" then return copy_value(state_value(dest_state(actor), "diffuse")) end
	if name == "GetZoom" or name == "GetZoomX" or name == "GetZoomY" or name == "GetZoomZ"
		or name == "GetRotationX" or name == "GetRotationY" or name == "GetRotationZ"
		or name == "GetSkewX" or name == "GetSkewY" then
		return state_value(dest_state(actor), state_key(name))
	end
	if name == "getaux" or name == "GetAux" then return state_value(actor.state, "aux") end
	if name:match("^[Gg]et") then
		local value = actor.state[state_key(name)]
		if value ~= nil then return value end
		if name:match("Width$") then return 256 end
		if name:match("Height$") then return 256 end
		return 0
	end
	if name:match("^Is") or name:match("^Has") then return false end

	-- Trace asset references are portable; the Lua call still uses its filename.
	if actor.class == "Sprite" and type((...)) == "string"
		and (name == "Load" or name == "LoadBackground" or name == "LoadBanner") then
		args[1] = source_path((...))
	end
	local event = emit("call", actor, event_operation(actor, name), args)
	if name == "SetUpdateFunction" then
		actor.update_fn = (...)
	elseif name == "name" then
		local value = (...)
		if type(value) ~= "string" and type(value) ~= "number" then error("name: string expected") end
		actor.__songlua_name = tostring(value)
		if actor.parent then
			local groups = {}
			for _, child in ipairs(actor.parent.children) do
				local child_name = child.__songlua_name
				groups[child_name] = groups[child_name] or {}
				table.insert(groups[child_name], child)
			end
			actor.parent.children_by_name = groups
		end
	elseif name == "set_mult_attrs_with_diffuse" then
		-- GETTER_SETTER_BOOL_METHOD uses lua_toboolean, so even numeric 0 is true.
		actor.state.mult_attrs_with_diffuse = not not (...)
	elseif name == "SetDrawFunction" then
		actor.draw_fn = (...)
		manual.actors[actor] = true
	elseif name == "Draw" then
		if manual.context then manual.draw(actor) end
	elseif name == "BeginRenderingTo" and actor.class == "RageTexture" then
		if manual.context then manual.begin(actor, (...)) end
	elseif name == "FinishRenderingTo" and actor.class == "RageTexture" then
		if manual.context then manual.finish(actor) end
	elseif name == "SetVertices" and actor.class == "ActorMultiVertex" then
		actor.state.vertices = copy_value((...))
	elseif name == "SetDrawState" and actor.class == "ActorMultiVertex" then
		actor.state.drawstate = copy_value((...))
	elseif name == "SetLineWidth" and actor.class == "ActorMultiVertex" then
		actor.state.linewidth = (...)
	elseif name == "Create" and actor.class == "ActorFrameTexture" then
        if rawget(actor, "allocated_texture") then
            lua.ReportScriptError("Can't Create an already created ActorFrameTexture")
            return actor
        end
        local texture_name = _ITG_TEXTURE_NAME(actor.state.texturename)
        if named_aft_textures[texture_name] then
            lua.ReportScriptError("ActorFrameTexture: Texture Name already in use.")
            return actor
        end
        local width, height = math.floor(tonumber(actor.state.width) or 1), math.floor(tonumber(actor.state.height) or 1)
        if width < 1 or height < 1 then
            lua.ReportScriptError("ActorFrameTexture: Cannot have width or height less than 1")
            return actor
        end
        local texture = typed_child(actor, "Texture", "RageTexture")
        local backing_width, backing_height = 1, 1
        while backing_width < width do backing_width = backing_width * 2 end
        while backing_height < height do backing_height = backing_height * 2 end
        texture.state = {
            path = texture_name, sourcewidth = width, sourceheight = height,
            imagewidth = width, imageheight = height, sourceframewidth = width, sourceframeheight = height,
            texturewidth = backing_width, textureheight = backing_height, numframes = 1,
            alpha = actor.state.enablealphabuffer == true, depth = actor.state.enabledepthbuffer == true,
            float = actor.state.enablefloat == true,
        }
        actor.allocated_texture = texture
        named_aft_textures[texture_name] = texture
    elseif name == "SetTextureName" and actor.class == "ActorFrameTexture" then
        local value = (...)
        if type(value) ~= "string" and type(value) ~= "number" then error("string expected") end
        actor.state.texturename = tostring(value)
    elseif actor.class == "ActorFrameTexture" and (name == "EnableAlphaBuffer" or name == "EnableDepthBuffer"
        or name == "EnableFloat" or name == "EnablePreserveTexture") then
        if type((...)) ~= "boolean" then error("boolean expected") end
        actor.state[name:lower()] = (...)
	elseif name == "SetTexture" or (actor.class == "Sprite"
		and (name == "Load" or name == "LoadBackground" or name == "LoadBanner")) then
		local texture = (...)
		if name == "LoadBackground" or name == "LoadBanner" then
			texture = tostring(texture)
		end
		actor.state.texture = texture
		local resource = actor_texture(actor)
		if type(resource) == "table" and resource.class == "RageTexture" then
			actor.state.width, actor.state.height = resource.state.sourceframewidth, resource.state.sourceframeheight
		elseif actor.class == "Sprite" then
			-- Sprite::Load replaces its size, even after a previous SetTexture.
			local path = texture_path(actor)
			if path then
				actor.state.width, actor.state.height = image_size(path)
				-- Retain runtime-loaded assets even when no frame draws the Sprite.
				file_observations.textures[#file_observations.textures + 1] = { actor = actor.id, method = name,
					path = source_path(path), exists = is_file(path) }
			end
		end
		-- These two LunaSprite methods return the existing top stack argument.
		if name == "LoadBackground" or name == "LoadBanner" then return select(select("#", ...), ...) end
	elseif name == "playcommand" or name == "PlayCommand" or name == "propagatecommand" then
		run_command_tree(actor, tostring((...)), select(2, ...))
    elseif name == "queuecommand" or name == "QueueCommand" then
        local command = tostring((...))
        if active_context and active_context.command == command .. "Command" then
            actor.recurring_commands = rawget(actor, "recurring_commands") or {}
            actor.recurring_commands[command] = true
        end
        begin_tween(actor, 0, "linear", command, event)
    elseif name == "queuemessage" or name == "QueueMessage" then
        -- Actor::QueueMessage uses the same native queue item as QueueCommand,
        -- with a ! marker consumed by UpdateTweening at dispatch.
        begin_tween(actor, 0, "linear", "!" .. tostring((...)), event)
	elseif name == "addcommand" or name == "AddCommand" then
		local command, fn = ...
		actor[tostring(command) .. "Command"] = fn
		local message = tostring(command):match("^(.*)Message$")
		if message then _ITG_MESSAGE_SUBSCRIBE(manual.ensure_message_subscriber(actor), message) end
	elseif name == "removecommand" or name == "RemoveCommand" then
		actor[tostring((...)) .. "Command"] = nil
	elseif name == "stoptweening" or name == "StopTweening" then
		actor.tweens = {}
	elseif name == "finishtweening" or name == "FinishTweening" then
		if #(actor.tweens or {}) > 0 then merge_tween_state(actor.state, actor.tweens[#actor.tweens].state) end
		actor.tweens = {}
	elseif name == "HurryTweening" then
		local factor = tonumber((...)) or 1
		for _, tween in ipairs(actor.tweens or {}) do
			tween.duration, tween.time_left = tween.duration * factor, tween.time_left * factor
		end
	elseif name == "sleep" then
		begin_tween(actor, (...), "linear", nil, event)
		begin_tween(actor, 0, "linear")
	elseif name == "hibernate" then
		actor.hibernate_seconds = _ITG_FLOAT(tonumber((...)) or 0)
	elseif name == "SetUpdateRate" then
		local number = tonumber((...))
		if number == nil then error("ActorFrame:SetUpdateRate: number expected") end
		local rate = _ITG_FLOAT(number)
		if rate <= 0 then
			error(string.format("ActorFrame:SetUpdateRate(%f) Update rate must be greater than 0.", rate))
		end
		-- The Lua binding rejects nonpositive floats; the C++ setter stores
		-- only rate > 0, so NaN returns without changing the existing rate.
		if rate > 0 then actor.update_rate = rate end
	elseif name == "linear" or name == "accelerate" or name == "decelerate" or name == "smooth" or name == "spring" then
		begin_tween(actor, (...), name, nil, event)
	elseif name == "bouncebegin" or name == "bounceend" then
		-- _fallback/Scripts/02 Actor.lua defines these aliases as Bezier tweens.
		local controls = name == "bouncebegin" and { 0,0, 0.42,-0.42, 2/3,0.3, 1,1 }
			or { 0,0, 1/3,0.7, 0.58,1.42, 1,1 }
		begin_tween(actor, (...), controls, nil, event)
	elseif name == "tween" then
		local duration, easing, controls = ...
		easing = tostring(easing or "linear"):lower()
		begin_tween(actor, duration, easing:match("bezier") and controls
			or easing:match("accelerate") and "accelerate"
			or easing:match("decelerate") and "decelerate"
			or easing:match("spring") and "spring" or "linear", nil, event)
	elseif name == "AddWrapperState" then
		local wrappers = rawget(actor, "wrappers") or {}
		rawset(actor, "wrappers", wrappers)
		local wrapper = external_actor(actor.path .. "/WrapperState" .. (#wrappers + 1), "ActorFrame")
		wrappers[#wrappers + 1] = wrapper
		return wrapper
	elseif name == "RemoveChild" then
		local removed = (...)
		for index = #actor.children, 1, -1 do if actor.children[index] == removed then table.remove(actor.children, index) end end
	elseif name == "RunCommandsOnChildren" or name == "runcommandsonleaves" then
		local command = (...)
		for _, child in ipairs(actor.children) do if type(command) == "function" then command(child) end end
	else
		local first = select(1, ...)
		if name == "x" or name == "y" or name == "z" or name == "zoomx" or name == "zoomy" or name == "zoomz"
			or name == "rotationx" or name == "rotationy" or name == "rotationz" or name == "skewx" or name == "skewy"
			or name == "cropleft" or name == "croptop" or name == "cropright" or name == "cropbottom"
			or name == "fadeleft" or name == "fadetop" or name == "faderight" or name == "fadebottom"
			or name == "aux" then set_tween_value(actor, name, first)
		elseif name == "addx" or name == "addy" or name == "addz" then add_tween_value(actor, name:sub(4), first)
		elseif name == "addrotationx" or name == "addrotationy" or name == "addrotationz" then
			add_tween_value(actor, name:sub(4), first)
		elseif name == "xy" then set_tween_value(actor, "x", first); set_tween_value(actor, "y", select(2, ...))
		elseif name == "xyz" then
			set_tween_value(actor, "x", first); set_tween_value(actor, "y", select(2, ...)); set_tween_value(actor, "z", select(3, ...))
		elseif name == "scaletocover" or name == "scaletofit" then
			local left, top, right, bottom = ...
			local width, height = actor_size(actor)
			if width and height and width > 0 and height > 0 then
				local dx, dy = right - left, bottom - top
				local zx, zy = math.abs(dx / width), math.abs(dy / height)
				local zoom = name == "scaletocover" and math.max(zx, zy) or math.min(zx, zy)
				if dx < 0 then set_tween_value(actor, "rotationy", 180) end
				if dy < 0 then set_tween_value(actor, "rotationx", 180) end
				set_tween_value(actor, "x", left + dx * (actor.state.halign or 0.5))
				set_tween_value(actor, "y", top + dy * (actor.state.valign or 0.5))
				for _, axis in ipairs({ "zoomx", "zoomy", "zoomz" }) do set_tween_value(actor, axis, zoom) end
			end
		elseif name == "stretchto" then
			local left, top, right, bottom = ...
			local width, height = actor_size(actor)
			set_tween_value(actor, "x", (left + right) / 2)
			set_tween_value(actor, "y", (top + bottom) / 2)
			set_tween_value(actor, "zoomx", (right - left) / width)
			set_tween_value(actor, "zoomy", (bottom - top) / height)
		elseif name == "zoomtowidth" or name == "zoomtoheight" then
			local width, height = actor_size(actor)
			local horizontal = name == "zoomtowidth"
			set_tween_value(actor, horizontal and "zoomx" or "zoomy", first / (horizontal and width or height))
		elseif name == "zoom" then
			set_tween_value(actor, "zoom", first); set_tween_value(actor, "zoomx", first)
			set_tween_value(actor, "zoomy", first); set_tween_value(actor, "zoomz", first)
		elseif name == "basezoom" then
			actor.state.basezoomx, actor.state.basezoomy, actor.state.basezoomz = first, first, first
		elseif name == "basezoomx" or name == "basezoomy" or name == "basezoomz" then
			-- Actor's base scale is immediate and independent of its tween scale.
			actor.state[name] = first
		elseif name == "baserotationx" or name == "baserotationy" or name == "baserotationz" then
			-- Actor::SetBaseRotation replaces an immediate field outside TweenState.
			actor.state[name] = _ITG_FLOAT(tonumber(first) or 0)
		elseif name == "zoomto" then
			-- Actor::ZoomTo writes destination scale, preserving intrinsic size.
			local width, height = actor_size(actor)
			set_tween_value(actor, "zoomx", first / width)
			set_tween_value(actor, "zoomy", select(2, ...) / height)
		elseif name == "SetSize" or name == "setsize" then actor.state.width, actor.state.height = first, select(2, ...)
		elseif name == "diffuse" or name == "glow" then set_tween_value(actor, name, _ITG_COLOR(...))
		elseif name == "diffuseupperleft" or name == "diffusetopedge" or name == "diffuseleftedge" then
            -- Projected color and GetDiffuse read native diffuse corner zero.
            set_tween_value(actor, "diffuse", _ITG_COLOR(...))
        elseif name == "diffuseupperright" or name == "diffuselowerright" or name == "diffuselowerleft"
            or name == "diffuserightedge" or name == "diffusebottomedge" then
            -- These setters leave native diffuse corner zero unchanged.
        elseif name == "diffusealpha" or name == "diffusecolor" then
			-- SetDiffuseAlpha and SetDiffuseColor update the destination color;
			-- neither creates an independent alpha channel that survives diffuse.
			local color = copy_value(state_value(dest_state(actor), "diffuse"))
			if name == "diffusealpha" then color[4] = _ITG_FLOAT(first)
			else
				local rgb = _ITG_COLOR(...)
				for index = 1, 3 do color[index] = rgb[index] end
			end
			set_tween_value(actor, "diffuse", color)
		elseif name == "horizalign" or name == "vertalign" then
			-- Actor's enum aliases set its immediate numeric alignment.
			local token = tostring(first):lower():gsub("^horizalign_", ""):gsub("^vertalign_", "")
			local values = name == "horizalign" and { left=0, center=0.5, right=1 } or { top=0, middle=0.5, bottom=1 }
			local value = values[token]
			if value ~= nil then actor.state[name == "horizalign" and "halign" or "valign"] = value end
		elseif name == "visible" then actor.state.visible = int_boolean(first)
		elseif name == "shadowlength" then
			-- Actor stores shadow offsets outside TweenState, even after sleep.
			actor.state.shadowlengthx, actor.state.shadowlengthy = _ITG_FLOAT(tonumber(first) or 0), _ITG_FLOAT(tonumber(first) or 0)
		elseif name == "shadowlengthx" or name == "shadowlengthy" then actor.state[name] = _ITG_FLOAT(tonumber(first) or 0)
		elseif name == "shadowcolor" then actor.state.shadowcolor = _ITG_COLOR(...)
		elseif name == "vanishpoint" then actor.state.vanishpoint = { first, select(2, ...) }
		elseif name == "effectcolor1" or name == "effectcolor2" then actor.state[name] = _ITG_COLOR(...)
		elseif name == "effectmagnitude" then actor.state.effectmagnitude = { first, select(2, ...), select(3, ...) }
		elseif name == "effectperiod" then
			local period = _ITG_FLOAT(tonumber(first) or 0)
			if period <= 0 then error("effectperiod must be positive") end
			actor.state.effectperiod = period
			actor.state.effecttiming = { _ITG_FLOAT(period / 2), 0, _ITG_FLOAT(period / 2), 0, 0 }
		elseif name == "effecttiming" or name == "effect_hold_at_full" then
			local period = tonumber(actor.state.effectperiod) or 1
			local timing = copy_value(actor.state.effecttiming or { period / 2, 0, period / 2, 0, 0 })
			if name == "effect_hold_at_full" then timing[4] = _ITG_FLOAT(tonumber(first) or 0)
			else
				-- Lua orders hold-at-zero fourth and optional hold-at-full fifth.
				for index, argument in ipairs({1, 2, 3, 5, 4}) do
					timing[index] = _ITG_FLOAT(tonumber((select(argument, ...))) or 0)
				end
			end
			local total = 0
			for index = 1, 5 do
				if timing[index] < 0 then error("effect timings must be nonnegative") end
				total = _ITG_FLOAT(total + timing[index])
			end
			if total <= 0 then error("effect timings must contain a positive interval") end
			actor.state.effecttiming, actor.state.effectperiod = timing, total
		elseif name == "pulse" then
			local magnitude = actor.state.effectmagnitude or { 0, 0, 10 }
			actor.state.effect, actor.state.effectperiod = "pulse", 2
			actor.state.effecttiming = { 1, 0, 1, 0, 0 }
			actor.state.effectmagnitude = { 0.5, 1, magnitude[3] or 10 }
		elseif name == "vibrate" then
			actor.state.effect, actor.state.effectepoch = "vibrate", current_seconds
			actor.state.effectmagnitude = { 10, 10, 10 }
		elseif name == "rainbow" then
			if actor.state.effect ~= "rainbow" then actor.state.spinclock = 0 end
			actor.state.effect, actor.state.effectepoch = "rainbow", current_seconds
			actor.state.effectperiod, actor.state.effecttiming = 2, { 1, 0, 1, 0, 0 }
		elseif name == "stopeffect" then
			actor.state.effect = nil
		elseif name == "bob" or name == "bounce" or name == "wag" then
			local changed = actor.state.effect ~= name
			local reset = name == "bounce" or changed
				or (name == "bob" and (tonumber(actor.state.effectperiod) or 1) ~= 2)
			if reset then actor.state.spinclock = 0 end
			actor.state.effect, actor.state.effectepoch = name, current_seconds
			if name ~= "bob" or reset then
				actor.state.effectperiod, actor.state.effecttiming = 2, { 1, 0, 1, 0, 0 }
			end
			actor.state.effectmagnitude = name == "wag" and { 0, 0, 20 } or { 0, 20, 0 }
        elseif COLOR_EFFECTS[name] then
            if actor.state.effect ~= name then actor.state.spinclock = 0 end
            actor.state.effect, actor.state.effectepoch = name, current_seconds
            actor.state.effectperiod, actor.state.effecttiming = 1, { .5,0,.5,0,0 }
            actor.state.effectcolor1, actor.state.effectcolor2 = unpack(copy_value(COLOR_EFFECTS[name]))
        elseif name == "spin" then
            actor.state.effect, actor.state.effectepoch = name, current_seconds
		elseif name == "settext" or name == "SetText" then actor.state.text = first
		else
			local key = state_key(name)
			actor.state[key] = first
			if key == "fov" then
				perspective_actors[actor] = first ~= nil and tonumber(first) ~= -1 and true or nil
			elseif key == "target" and actor.class == "ActorProxy" then
				for player = 1, 2 do player_proxy_sets[player][actor] = nil end
				for player = 1, 2 do
					if tracked_players and first == tracked_players[player] then player_proxy_sets[player][actor] = true end
				end
			end
		end
	end
	if event then
		event.detail = event.detail or {}
		event.detail.headless_tween_time_left = tween_time_left(actor)
	end
	return actor
end

-- Class and instance lookups must make the same feature decision. Native C
-- methods expect real userdata, so forward valid methods to semantic actors.
do
local actor_classes = actor_mt.classes
for class, native in pairs(actor_classes.native) do
    local methods = {}
    actor_classes[class] = methods
    _G[class] = methods
    -- Luna registers own methods eagerly; they shadow inherited methods even
    -- when the base class is overridden later by Lua.
    for name, method in pairs(native) do
        if type(method) == "function" then
            methods[name] = function(self, ...) return actor_call(self, name, ...) end
        end
    end
    for name in pairs(_ITG_ACTOR_HELPERS[class] or {}) do
        if methods[name] == nil then
            methods[name] = function(self, ...) return actor_call(self, name, ...) end
        end
    end
    setmetatable(methods, { __index = function(_, name)
        local base = actor_classes.bases[class]
        if base then return actor_classes[base][name] end
    end })
end
-- Quad has Sprite's Lua type (Quad.h), without a separate Lua method table.
actor_classes.Quad = actor_classes.Sprite
-- _fallback/Scripts/02 Actor.lua helper bodies use native actor primitives.
function BitmapText:PixelFont()
    self:SetTextureFiltering(false)
    return self
end
function BitmapText:Stroke(c)
    self:strokecolor(c)
    return self
end
function BitmapText:NoStroke()
    self:strokecolor(color("0,0,0,0"))
    return self
end
function BitmapText:settextf(...)
    self:settext(string.format(...))
    return self
end
function BitmapText:DiffuseAndStroke(diffuseC, strokeC)
    self:diffuse(diffuseC)
    self:strokecolor(strokeC)
    return self
end
end

actor_mt.__index = function(actor, name)
    if name == "Name" then return nil end
    local methods = actor_mt.classes[actor.class]
    if methods then return methods[name] end
    -- Unlinked class adapters still need a source-backed registration audit.
    if name == "GetChildAt" then return nil end
    -- These classes inherit LunaBitmapText; native ActorFrames have no GetText.
    if name == "GetText" and not (actor.class == "BitmapText" or actor.class == "RollingNumbers"
        or actor.class == "BPMDisplay" or actor.class == "HelpDisplay" or actor.class == "ActiveAttackList"
        or actor.class == "ScoreDisplayAliveTime" or actor.class == "ScoreDisplayCalories"
        or actor.class == "DeviceList" or actor.class == "InputList") then return nil end
	return function(self, ...) return actor_call(self, name, ...) end
end
actor_mt.__tostring = function(actor)
    local class = ({Quad = "Sprite", Screen = "ScreenGameplay"})[actor.class] or actor.class
    return class .. " (" .. actor.identity .. ")"
end

local function command_info(fn)
	local info = debug.getinfo(fn, "Sl") or {}
	return source_path(info.source or ""), info.linedefined or 0, info.lastlinedefined or 0
end

run_command = function(actor, name, params)
	local fn = rawget(actor, name .. "Command")
	if type(fn) ~= "function" then return end
	command_count = command_count + 1
	if command_count > harness.max_commands then
		dropped_events = dropped_events + 1
		return
	end
	local source, line = command_info(fn)
	local prior = active_context
	active_context = { definition_id = actor.definition_id, command = name .. "Command", source = source, line = line, recurring = (rawget(actor, "recurring_commands") or {})[name] }
	emit("command", actor, "command.begin", {}, { name = name .. "Command" })
	local ok, message = pcall(fn, actor, params)
	if ok then
		emit("command", actor, "command.end", {}, { name = name .. "Command" })
	else
		record_runtime_error(actor, "command.error", name .. "Command", message)
	end
	active_context = prior
end

function manual.dispatch_subscriber(actor, name, params)
	if manual.active_message_dispatch then
		local ids = manual.active_message_dispatch.actor_ids
		ids[#ids + 1] = actor.id
	end
	run_command(actor, name .. "Message", params)
end

manual.ensure_message_subscriber = function(actor)
	local subscriber = rawget(actor, "native_subscriber")
	if not subscriber then
		subscriber = _ITG_MESSAGE_REGISTER(actor, manual.dispatch_subscriber)
		actor.native_subscriber = subscriber
	end
	return subscriber
end

run_command_tree = function(actor, name, params)
	run_command(actor, name, params)
	for _, child in ipairs(actor.children or {}) do
		run_command_tree(child, name, params)
	end
end

local function definition_properties(definition)
	local out = {}
	for key, value in pairs(definition) do
		local command = type(key) == "string" and key:match("Command$")
		if type(key) == "string" and not command and key ~= "Class" and key ~= "Name" and key ~= "children" and key:sub(1, 1) ~= "_" then
			out[key] = safe_value(value, 0, {})
		end
	end
	return out
end

local function definition_children(definition)
	local out = {}
	local indexes = {}
	for key in pairs(definition) do
		if type(key) == "number" and key > 0 and key % 1 == 0 then indexes[#indexes + 1] = key end
	end
	table.sort(indexes)
	for _, index in ipairs(indexes) do out[#out + 1] = definition[index] end
	if type(definition.children) == "table" then
		indexes = {}
		for key in pairs(definition.children) do
			if type(key) == "number" and key > 0 and key % 1 == 0 then indexes[#indexes + 1] = key end
		end
		table.sort(indexes)
		for _, index in ipairs(indexes) do out[#out + 1] = definition.children[index] end
	end
	return out
end

local function register_definition(definition, parent, layer_index)
	if type(definition) ~= "table" then return nil end
	local existing = definition_by_table[definition]
	if existing then return existing end
	local record = {
		id = "def-" .. string.format("%04d", #definitions + 1),
		class = tostring(definition.Class or "Actor"),
		name = definition.Name and tostring(definition.Name) or nil,
		source = source_path(definition._Source or ""),
		line = tonumber(definition._Line) or 0,
		properties = definition_properties(definition),
		commands = {}, children = {}, runtime_actors = {},
		parent_id = parent and parent.id or nil,
		layer_index = layer_index,
	}
	definition_by_table[definition] = record
	definitions[#definitions + 1] = record
	local command_names = {}
	for key, value in pairs(definition) do
		if type(key) == "string" and key:match("Command$") and type(value) == "function" then command_names[#command_names + 1] = key end
	end
	table.sort(command_names)
	for _, name in ipairs(command_names) do
		local source, line, last_line = command_info(definition[name])
		record.commands[#record.commands + 1] = { name = name, source = source, line = line, last_line = last_line }
	end
	for index, child in ipairs(definition_children(definition)) do
		if type(child) == "table" then
			local child_record = register_definition(child, record, index)
			if child_record then record.children[#record.children + 1] = { layer_index = index, definition_id = child_record.id } end
		end
	end
	return record
end

local function instantiate(definition, parent)
	local record = definition_by_table[definition]
	local instance = #record.runtime_actors + 1
	local id = record.id .. (instance == 1 and "" or "#" .. instance)
	local actor = {
		__actor = true, id = id, path = id, __songlua_name = record.name or "",
		class = record.class, definition_id = record.id, parent = parent,
		children = {}, children_by_name = {}, wrappers = {}, state = {}, tweens = {},
		source = record.source,
	}
	actor.identity = tostring(actor):match("^table: (.+)$")
	for key, value in pairs(definition) do
		if type(key) == "string" and key:match("Command$") and type(value) == "function" then actor[key] = value end
	end
    for key, value in pairs(record.properties) do
        -- ActorFrame::LoadFromNode reads the case-sensitive FOV attribute.
        if key:lower() ~= "fov" or key == "FOV" then actor.state[key:lower()] = value end
    end
    if actor.class == "Model" then
        local function piece(key)
            local value = rawget(definition, key)
            if value == nil or value == "" then return nil end
            local path = normalize(value)
            if path:sub(1, 1) ~= "/" and not path:match("^%a:/") then
                path = normalize((definition._ModelDirectory or song_dir) .. "/" .. path)
            end
            return path
        end
        local handle, message = _ITG_MODEL_LOAD(piece("Meshes"), piece("Materials"), piece("Bones"))
        if not handle then error(message, 0) end
        actor.native_model = handle
    end
    if actor.class == "ActorFrameTexture" then
        manual.aft_counter = (manual.aft_counter or 0) + 1
        actor.state.texturename = "ActorFrameTexture " .. manual.aft_counter
    end
	if actor.state.fov ~= nil and tonumber(actor.state.fov) ~= -1 then perspective_actors[actor] = true end
	setmetatable(actor, actor_mt)
	record.runtime_actors[#record.runtime_actors + 1] = actor.id
	actor_by_id[actor.id] = actor
	runtime_actors[#runtime_actors + 1] = { id = actor.id, path = actor.path, name = actor.__songlua_name, definition_id = record.id, parent_id = parent and parent.id or nil }
	local subscriber = manual.ensure_message_subscriber(actor)
	for _, command in ipairs(record.commands) do
		local message = command.name:match("^(.*)MessageCommand$")
		if message then _ITG_MESSAGE_SUBSCRIBE(subscriber, message) end
	end
	for _, child_definition in ipairs(definition_children(definition)) do
		if type(child_definition) == "table" then
			local child = instantiate(child_definition, actor)
			actor.children[#actor.children + 1] = child
			local child_name = child.__songlua_name
			actor.children_by_name[child_name] = actor.children_by_name[child_name] or {}
			table.insert(actor.children_by_name[child_name], child)
		end
	end
	return actor
end

local function visit(actor, fn)
	fn(actor)
	for _, child in ipairs(actor.children or {}) do visit(child, fn) end
end

broadcast = function(name, params)
	if type(params) == "table" and getmetatable(params) == actor_mt then
		error("bad argument #2 to 'Broadcast' (table or nil expected, got userdata)", 0)
	end
	name = _ITG_MESSAGE_CHECK(name, params)
	emit("message", nil, "MessageManager.Broadcast", safe_args(name, params))
	local dispatch = { name = tostring(name), beat = current_beat, seconds = current_seconds, actor_ids = {} }
	if #manual.message_dispatches < harness.max_events then
		manual.message_dispatches[#manual.message_dispatches + 1] = dispatch
	else
		dropped_events = dropped_events + 1
	end
	local previous = manual.active_message_dispatch
	manual.active_message_dispatch = dispatch
	local ok, message = pcall(_ITG_MESSAGE_BROADCAST, name, params)
	manual.active_message_dispatch = previous
	if not ok then error(message, 0) end
end

MESSAGEMAN = { Broadcast = function(self, name, params) broadcast(name, params); return self end }

local option_query_error
local function update_native_options(options, name, ...)
	-- Older captures permit custom modifier names. Preserve their events, but
	-- refuse a later query if its native state could not be reproduced.
	local ok, message = pcall(_ITG_OPTIONS_UPDATE, options.native_index, name, ...)
	if not ok then option_query_error = message end
end
local player_options_mt = {}
local function option_returns(...) return { n = select("#", ...), ... } end
local function indexed_option_noops(options, text)
	local out = {}
	local methods = { confusionoffset = "ConfusionOffset", movex = "MoveX", movey = "MoveY" }
	for part in tostring(text):gmatch("[^,]+") do
		local key = part:lower():match("(%S+)%s*$")
		local prefix, suffix = (key or ""):match("^(.-)(%d+)$")
		local method, column = methods[prefix], tonumber(suffix)
		if method and (column < 1 or column > 16) then
			local snapshot = { key = key, unchanged = _ITG_USING_MODIFIER(options.native_index, part), values = {} }
			-- Query the linked PlayerOptions fields, not the authored invalid level.
			if prefix == "confusionoffset" then
				snapshot.values[#snapshot.values + 1] = { prefix, _ITG_OPTIONS_UPDATE(options.native_index, method) }
			end
			for index = 1, column_count do
				snapshot.values[#snapshot.values + 1] = { prefix .. index, _ITG_OPTIONS_UPDATE(options.native_index, method .. index) }
			end
			out[#out + 1] = snapshot
		end
	end
	return #out > 0 and out or nil
end
function manual.numeric_options(options)
	local values = {}
	for _, method in ipairs(_ITG_PLAYER_OPTION_FLOATS) do
		local amount, speed = _ITG_OPTIONS_UPDATE(options.native_index, method, nil)
		if type(amount) == "number" then
			values[#values + 1] = { method:lower(), amount, speed }
		end
	end
	return values
end
function manual.assignment_options(options)
	local values = manual.numeric_options(options)
	for _, method in ipairs({ "StealthType", "StealthPastReceptors", "Cosecant", "DizzyHolds", "ZBuffer" }) do
		values[#values + 1] = { method:lower(), _ITG_OPTIONS_UPDATE(options.native_index, method) and 1 or 0 }
	end
	local mode = _ITG_OPTIONS_UPDATE(options.native_index, "ModTimerSetting")
	local modes = { ModTimerType_Game = 0, ModTimerType_Beat = 1, ModTimerType_Song = 2, ModTimerType_Default = 3 }
	assert(modes[mode] ~= nil, "unavailable native modifier timer enum")
	values[#values + 1] = { "modtimersetting", modes[mode] }
	return values
end
local function rejected_option_parts(options, text)
	local parts = _ITG_OPTIONS_REJECTED(options.native_index, text)
	if #parts == 0 then return nil end
	local values = manual.numeric_options(options)
	for _, part in ipairs(parts) do part.values = values end
	return parts
end
player_options_mt.__index = function(options, name)
	if not rawget(options, "allow_unknown") and not _ITG_PLAYER_OPTION_METHODS[name] then return nil end
	if name == "GetReversePercentForColumn" then return function(self, column)
		local col = math.floor(tonumber(column) or 0)
		if col < 0 or col > column_count then return nil end
		local function amount(mod) return _ITG_OPTIONS_UPDATE(self.native_index, mod, nil) end
		-- Native GetReversePercentForColumn needs a full GameState style.
		-- Apply its exact column composition to the linked option amounts.
		local value = _ITG_FLOAT(amount("Reverse") + amount("Reverse" .. (col + 1)))
		if col >= math.floor(column_count / 2) then value = _ITG_FLOAT(value + amount("Split")) end
		if col % 2 == 1 then value = _ITG_FLOAT(value + amount("Alternate")) end
		local first = math.floor(column_count / 4)
		if col >= first and col <= column_count - 1 - first then value = _ITG_FLOAT(value + amount("Cross")) end
		if value > 2 then value = _ITG_FLOAT(value % 2) end
		if value > 1 then value = _ITG_FLOAT(2 - value) end
		return value
	end end
	return function(self, ...)
		if self.kind == "PlayerOptions" and (_ITG_PLAYER_OPTION_METHODS[name] and name ~= "FromString") then
			local count = select("#", ...)
			local bool_option = _ITG_PLAYER_OPTION_BOOLS[name] and name ~= "Overhead"
			local bool_write = bool_option and count > 0 and type(select(1, ...)) == "boolean"
			local previous = bool_write and _ITG_OPTIONS_UPDATE(self.native_index, name) or nil
			local skin_write = name == "NoteSkin" and count > 0
				and (type(select(1, ...)) == "string" or type(select(1, ...)) == "number")
			local skin_before = skin_write and _ITG_OPTIONS_UPDATE(self.native_index, "NoteSkin") or nil
			local event
			-- A nil enum argument only queries; it may request chaining as well.
			-- BOOL_INTERFACE only writes when its first argument is a boolean.
			if count > 0 and select(1, ...) ~= nil and (not bool_option or type(select(1, ...)) == "boolean")
				and (name ~= "ModTimerSetting" or select(1, ...) ~= nil) then
				event = emit("modifier", self, self.kind .. "." .. name, safe_args(...),
					bool_write and { boolean_option = {} } or skin_write and { noteskin_option = {} } or nil)
			end
			-- Native getters return amounts and speeds (or inactive alias nils).
			-- Setters return previous values unless the last boolean requests chaining.
			local values = option_returns(_ITG_OPTIONS_UPDATE(self.native_index, name, ...))
			if bool_write and event then
				event.detail = { boolean_option = {
					previous = previous == true,
					current = _ITG_OPTIONS_UPDATE(self.native_index, name),
					chained = count >= 2 and type(select(2, ...)) == "boolean",
				} }
			end
			if skin_write and event then
				event.detail = { noteskin_option = {
					previous = skin_before,
					current = _ITG_OPTIONS_UPDATE(self.native_index, "NoteSkin"),
				} }
			end
			if bool_option then
				if count >= 2 and type(select(2, ...)) == "boolean" then return self end
			elseif count > 0 and type(select(count, ...)) == "boolean" and select(count, ...) then return self end
			return unpack(values, 1, values.n)
		end
		if name:match("^Get") or select("#", ...) == 0 then
			local value = self.values[name]
			if value ~= nil then return value end
			if self.kind == "PlayerOptions" and _ITG_PLAYER_OPTION_BOOLS[name] then return false end
			if name == "XMod" or self.kind == "SongOptions" and name == "MusicRate" then return 1 end
			if name == "CMod" or name == "MMod" then return nil end
			return 0
		end
		local skin_before = self.kind == "PlayerOptions" and name == "FromString"
			and _ITG_OPTIONS_UPDATE(self.native_index, "NoteSkin") or nil
		local event = emit("modifier", self, self.kind .. "." .. name, safe_args(...),
			skin_before and { noteskin_option = {} } or nil)
		update_native_options(self, name, ...)
		if self.kind == "PlayerOptions" and name == "FromString" then
			local noops = indexed_option_noops(self, (...))
			if event then event.detail = {
				indexed_noops = noops,
				rejected_parts = rejected_option_parts(self, (...)),
				noteskin_option = {
					previous = skin_before,
					current = _ITG_OPTIONS_UPDATE(self.native_index, "NoteSkin"),
					parts = _ITG_OPTIONS_SKINS(self.native_index, (...)),
				},
			} end
		end
		self.values[name] = (...)
		return self
	end
end

local function make_options(path, kind, allow_unknown)
	local index = kind == "SongOptions" and -1 or (path:find("PLAYER_2", 1, true) and 1 or 0)
	return setmetatable({ path = path, id = path, kind = kind, values = {}, native_index = index, allow_unknown = allow_unknown }, player_options_mt)
end

local player_options = { PLAYER_1 = make_options("player-state:PLAYER_1/options:ModsLevel_Song", "PlayerOptions"), PLAYER_2 = make_options("player-state:PLAYER_2/options:ModsLevel_Song", "PlayerOptions") }
local song_options = make_options("song-options:ModsLevel_Song", "SongOptions", true)

local song_position
local player_state_mt = {}
player_state_mt.__index = function(state, name)
    if name == "GetSongPosition" then return function() return song_position end end
	if name == "GetPlayerOptions" then return function(self) return player_options[self.player] end end
	if name == "GetPlayerOptionsString" then return function(self)
		return _ITG_OPTIONS_UPDATE(player_options[self.player].native_index, "GetString")
	end end
	if name == "SetPlayerOptions" then return function(self, level, value)
		local options = player_options[self.player]
		local before = _ITG_OPTIONS_UPDATE(options.native_index, "NoteSkin")
		local event = emit("modifier", self, "PlayerState.SetPlayerOptions", safe_args(level, value), { noteskin_option = {} })
		player_options[self.player].values = {}
		update_native_options(player_options[self.player], "SetPlayerOptions", value)
		if event then event.detail = { numeric_options = manual.assignment_options(options), noteskin_option = {
			previous = before,
			current = _ITG_OPTIONS_UPDATE(options.native_index, "NoteSkin"),
			parts = _ITG_OPTIONS_SKINS(options.native_index, value),
		} } end
		return self
	end end
	return function(self) return self end
end

local player_states = {
	PLAYER_1 = setmetatable({ id = "player-state:PLAYER_1", path = "player-state:PLAYER_1", player = "PLAYER_1" }, player_state_mt),
	PLAYER_2 = setmetatable({ id = "player-state:PLAYER_2", path = "player-state:PLAYER_2", player = "PLAYER_2" }, player_state_mt),
}

do
local function music_seconds(visible)
    -- Call the compiled SongPosition Lua binding. Trace timestamps remain
    -- relative to beat zero; this public API uses the native music timestamp.
    if not _ITG_NATIVE_SONG_POSITION then return _ITG_FLOAT(current_seconds) end
    local native = _ITG_NATIVE_SONG_POSITION(current_seconds)
    if visible then return native:GetMusicSecondsVisible() end
    return native:GetMusicSeconds()
end

song_position = {
	GetSongBeat = function() return _ITG_FLOAT(current_beat) end,
	GetSongBeatVisible = function() return _ITG_FLOAT(current_beat) end,
	GetMusicSeconds = function() return music_seconds(false) end,
	GetMusicSecondsVisible = function() return music_seconds(true) end,
	GetCurBPS = function() return current_bps end,
	GetFreeze = function() return current_freeze end,
	GetDelay = function() return current_delay end,
}
end

local song = {
	GetBackgroundPath = function() return harness.background_path end,
	GetSongDir = function() return song_dir .. "/" end,
	GetDisplayFullTitle = function() return harness.title end,
	GetDisplayMainTitle = function() return harness.title end,
	GetMainTitle = function() return harness.title end,
	GetDisplayBpms = function() return { harness.bpm, harness.bpm } end,
	GetTimingData = function() return _ITG_TIMING_DATA(0) end,
    GetSongBPS = function() return harness.bpm / 60 end,
    GetFirstSecond = function() return harness.first_second or 0 end,
    GetLastSecond = function() return harness.last_second or harness.max_beat * 60 / harness.bpm end,
    MusicLengthSeconds = function()
        if harness.music_length == nil then
            local length, message = _ITG_MUSIC_LENGTH(assert(harness.music_path, "native music path unavailable"))
            assert(length, message)
            -- Song::ReCalculateStepStatsAndLastSecond corrects short music.
            if length < harness.last_second - 10 then length = harness.last_second end
            harness.music_length = length
            file_observations.reads[#file_observations.reads + 1] = { path=source_path(harness.music_path), exists=true, generated=false }
        end
        return harness.music_length
    end,
    GetMusicPath = function() return harness.music_path end,
}

local all_steps = {}
for index, chart in ipairs(harness.steps or { harness }) do
	local steps_type = chart.steps_type
	if not steps_type:match("^StepsType_") then
		steps_type = "StepsType_" .. steps_type:gsub("-", "_"):gsub("(%a)(%w*)", function(first, rest)
			return first:upper() .. rest
		end)
	end
	all_steps[index] = {
		GetDifficulty = function() return chart.difficulty end,
		GetStepsType = function() return steps_type end,
        GetDescription = function() return chart.description end,
        GetAuthorCredit = function() return chart.author_credit or "" end,
        GetChartName = function() return chart.chart_name or "" end,
		GetMeter = function() return chart.meter or 0 end,
		GetTimingData = function() return _ITG_TIMING_DATA(index) end,
		GetNoteData = function() return {} end,
	}
end
local steps = assert(all_steps[harness.current_steps or 1], "missing current Steps")
_ITG_CURRENT_STEPS = { PLAYER_1 = steps, PLAYER_2 = steps }
song.GetAllSteps = function()
	local result = {}
	for index, chart in ipairs(all_steps) do result[index] = chart end
	return result
end
song.GetStepsByStepsType = function(_, steps_type)
	steps_type = _ITG_STEPS_TYPE(steps_type)
	local result = {}
	-- Native Enum::Check accepts the Lua enum name, not the simfile spelling.
	for _, chart in ipairs(all_steps) do
		if chart:GetStepsType() == steps_type then result[#result+1] = chart end
	end
	return result
end
song.GetOneSteps = function(_, steps_type, difficulty)
    steps_type = _ITG_STEPS_TYPE(steps_type)
    difficulty = _ITG_DIFFICULTY(difficulty)
    for _, chart in ipairs(all_steps) do
        if chart:GetStepsType() == steps_type and chart:GetDifficulty() == difficulty then return chart end
    end
end
song.GetEasiestStepsDifficulty = function() return harness.difficulty end
-- The isolated harness library contains only the current song.
SONGMAN = { GetSongGroupNames = function() return {} end,
    FindSong = function(_, query)
	local key = normalize(query):lower():gsub("/$", "")
	if key ~= "" and song_dir:lower():sub(-#key) == key then return song end
	return nil
end }

local style = {
	GetName = function() return style_name end,
	ColumnsPerPlayer = function() return column_count end,
	GetColsPerPlayer = function() return column_count end,
    GetStyleType = function() return is_double and "StyleType_OnePlayerTwoSides" or (style_name == "routine" and "StyleType_TwoPlayersSharedSides" or "StyleType_TwoPlayersTwoSides") end,
	GetWidth = function() return column_count * 64 end,
}
local game = { GetName = function() return "dance" end }

local function player_key(player)
	if player == "PLAYER_2" or player == "PlayerNumber_P2" or player == 1 then return "PLAYER_2" end
	return "PLAYER_1"
end

local game_env = {}
function getenv(name) return game_env[name] end
function setenv(name, value) game_env[name] = value end
GAMESTATE = {
	Env = function() return game_env end,
	PlayerIsUsingModifier = function(_, player, modifier)
		if option_query_error then error("native modifier state unavailable: " .. option_query_error) end
		return _ITG_USING_MODIFIER(player_key(player) == "PLAYER_2" and 1 or 0, modifier)
	end,
	GetSongBeat = function() return song_position:GetSongBeat() end,
	GetCurMusicSeconds = function() return song_position:GetMusicSeconds() end,
	GetSongPosition = function() return song_position end,
	GetSongBPS = function() return current_bps end,
	GetCurBPS = function() return current_bps end,
	GetCurrentSong = function() return song end,
	GetCurrentSteps = function(_, player) return _ITG_CURRENT_STEPS[player_key(player)] end,
	GetCurrentStyle = function() return style end,
	GetCurrentGame = function() return game end,
	GetPlayerState = function(_, player) return player_states[player_key(player)] end,
	GetEnabledPlayers = function() return is_double and { PLAYER_1 } or { PLAYER_1, PLAYER_2 } end,
	GetNumPlayersEnabled = function() return is_double and 1 or 2 end,
	GetHumanPlayers = function() return is_double and { PLAYER_1 } or { PLAYER_1, PLAYER_2 } end,
	IsPlayerEnabled = function(_, player) return not is_double or player_key(player) == "PLAYER_1" end,
	IsHumanPlayer = function(_, player) return not is_double or player_key(player) == "PLAYER_1" end,
	GetSongOptionsObject = function() return song_options end,
	GetSongOptions = function() return _ITG_OPTIONS_UPDATE(-1, "GetString") end,
    GetSongOptionsString = function() return _ITG_OPTIONS_UPDATE(-1, "GetString") end,
    GetMasterPlayerNumber = function() return PLAYER_1 end,
    GetCoinMode = function() return "CoinMode_Home" end,
    GetPremium = function() return "Premium_Off" end,
    IsEventMode = function() return PREFSMAN:GetPreference("EventMode") end,
	GetEasiestStepsDifficulty = function() return harness.difficulty end,
    SetCurrentSteps = function(_, player, chart)
        assert(chart, "SetCurrentSteps requires a Steps object")
        _ITG_CURRENT_STEPS[player_key(player)] = chart
        emit("call", nil, "GameState.SetCurrentSteps", safe_args(player, chart:GetStepsType(), chart:GetDifficulty(), chart:GetDescription()))
    end,
    SetCurrentStyle = function(_, name)
        assert(name == "single" or name == "double" or name == "couple" or name == "routine", "unsupported dance style: " .. tostring(name))
        style_name = name
        is_double = name == "double"
        column_count = (name == "double" or name == "routine") and 8 or 4
        emit("call", nil, "GameState.SetCurrentStyle", safe_args(name))
    end,
    ApplyStageModifiers = function(_, player, modifiers)
        local key = player_key(player)
        _ITG_APPLY_STAGE_MODIFIERS(key == "PLAYER_2" and 1 or 0, modifiers)
        emit("modifier", player_states[key], "GameState.ApplyStageModifiers", safe_args(player, modifiers))
    end,
	ApplyGameCommand = function(_, command) emit("modifier", nil, "GameState.ApplyGameCommand", safe_args(command)) end,
}

local top_screen = external_actor("ScreenGameplay", "Screen")
tracked_players = { typed_child(top_screen, "PlayerP1", "Player") }
if not is_double then tracked_players[2] = typed_child(top_screen, "PlayerP2", "Player") end
for _, name in ipairs({ "Overlay", "Underlay", "SongBackground", "SongForeground", "In" }) do
    typed_child(top_screen, name, "ActorFrame")
end
-- ScreenWithMenuElements keeps In as a separate Transition child. Simply
-- Love's in/default.lua retains its Stage/Event text after the visual lead-in
-- ends, so explicitly drawing In during a song is not an empty operation.
-- ScreenGameplay.cpp names these engine children. Simply-Love-SM5's
-- [ScreenGameplay] On commands hibernate them forever and draw its own HUD
-- under Underlay. Hibernation does not change the Actor visible flag.
for _, name in ipairs({ "LifeP1", "LifeP2", "ScoreP1", "ScoreP2", "StepsDisplayP1", "StepsDisplayP2" }) do
    actor_child(top_screen, name).hibernate_seconds = math.huge
end
local fallback_player_x = {
	math.floor((0.85 / 3) * harness.screen_width),
	math.floor((2.15 / 3) * harness.screen_width),
}
for index, player in ipairs(tracked_players) do
	player.state.x = is_double and harness.screen_width / 2 or fallback_player_x[index]
	player.state.y = harness.screen_height / 2
	typed_child(player, "NoteField", "NoteField").state.y = 10
    -- Player::Init renames the Simply Love frame Judgment; its graphic is a
    -- Sprite named JudgmentWithOffsets. Unknown children must stay absent.
    local judgment = actor_child(player, "Judgment")
    judgment.child_lookup_exact = true
    local sprite = typed_child(judgment, "JudgmentWithOffsets", "Sprite")
    sprite.state.animate, sprite.state.visible = false, false
    -- The theme loads its profile-selected graphic during Init, before the
    -- song tree runs. This initial resource is not a song Sprite.Load event.
    sprite.state.texture = _ITG_SONG_JUDGMENT
end
SCREENMAN = {
	GetTopScreen = function() return top_screen end,
	SystemMessage = function(_, message) emit("call", nil, "ScreenManager.SystemMessage", safe_args(message)) end,
	SetNewScreen = function(_, screen) emit("call", nil, "ScreenManager.SetNewScreen", safe_args(screen)) end,
}

DISPLAY = {
	GetDisplayWidth = function() return harness.display_width end,
	GetDisplayHeight = function() return harness.display_height end,
	GetFPS = function() return 60 end,
	GetVsync = function() return true end,
    SupportsRenderToTexture = function() return true end,
}

-- The host advertises Simply Love; use that checked-out theme's [Player]
-- player metrics, rather than zero (which shifts Lua fields and creates
-- infinite DrawSize strings). Simply-Love-SM5-8ms-iamchris4life/metrics.ini:
-- receptor Y -125/145, draw distances _screen.h*1.5/-130.
local function theme_metric(group, name)
    if group == "Common" and name == "ScreenHeight" then return harness.screen_height end
    if group == "Common" and name == "DefaultNoteSkinName" then return "cel" end
    if group == "ScreenGameplay" then
        -- Simply-Love-SM5/metrics.ini keeps single fields a quarter of the
        -- clamped logical width from center; double/shared fields are centered.
        local player, style = name:match("^PlayerP([12])(.+)X$")
        if style == "OnePlayerOneSide" or style == "TwoPlayersTwoSides" then
            local offset = math.max(640, math.min(854, harness.screen_width)) * 0.25
            return harness.screen_width / 2 + (player == "1" and -offset or offset)
        end
        if style == "OnePlayerBothSides" or style == "OnePlayerTwoSides"
            or style == "TwoPlayersSharedSides" then
            return harness.screen_width / 2
        end
    end
    if group == "Player" and name == "ReceptorArrowsYStandard" then return -125 end
    if group == "Player" and name == "ReceptorArrowsYReverse" then return 145 end
    if group == "Player" and name == "DrawDistanceBeforeTargetsPixels" then return harness.screen_height * 1.5 end
    if group == "Player" and name == "DrawDistanceAfterTargetsPixels" then return -130 end
end

THEME = {
	GetCurThemeName = function() return "Simply Love" end,
	GetMetric = function(_, group, name) return theme_metric(group, name) or 0 end,
	GetMetricB = function() return false end,
	HasMetric = function(_, group, name) return theme_metric(group, name) ~= nil end,
    GetString = function(_, group, name) return tostring(name) end,
	GetPathG = function(_, _, path) return tostring(path or "") end,
	GetPathB = function(_, _, path) return tostring(path or "") end,
	GetPathS = function(_, _, path) return tostring(path or "") end,
	GetPathF = function(_, _, path) return tostring(path or "") end,
}

-- Player.cpp declares TimingWindowAdd as a mutable float with default zero.
local timing_window_add = 0
PREFSMAN = { GetPreference = function(_, name)
	if tostring(name):lower() == "timingwindowadd" then return timing_window_add end
	if name == "EventMode" then return true end
    if name == "CoinMode" then return "CoinMode_Home" end
	if name == "VideoRenderers" then return "opengl" end
    if name == "LastSeenVideoDriver" then return "" end
	if name == "DisplayWidth" then return harness.display_width end
	if name == "DisplayHeight" then return harness.display_height end
	if name == "DisplayAspectRatio" then return harness.display_width / harness.display_height end
	if name == "GlobalOffsetSeconds" then return 0 end
    if name == "Theme" then return "Simply Love" end
	-- PrefsManager.cpp defaults to BFM_CoverPreserve.
	if name == "BackgroundFitMode" then return "BackgroundFitMode_CoverPreserve" end
	-- Background.cpp declares this as a float, used by numeric diffuse calls.
	if name == "BGBrightness" then return 0.7 end
	return false
end,
SetPreference = function(self, name, value)
	if tostring(name):lower() ~= "timingwindowadd" then
		error("headless SetPreference does not support " .. tostring(name))
	end
	-- Preference<float>::SetFromStack uses Lua's numeric conversion, then float.
	timing_window_add = _ITG_FLOAT(tonumber(value) or 0)
	return self
end }
local harness_profile = {
	GetLastUsedHighScoreName = function() return "" end,
	GetDisplayName = function() return "Guest" end,
	SetLastUsedHighScoreName = function() end,
}
PROFILEMAN = {
	IsPersistentProfile = function() return false end,
	GetNumLocalProfiles = function() return 0 end,
	GetProfile = function() return harness_profile end,
    GetPlayerName = function() return "" end,
}
STATSMAN = { GetCurStageStats = function() return external_actor("stage-stats", "StageStats") end }
FILEMAN = { DoesFileExist = function(_, path) return is_file(normalize(path)) end,
    GetDirListing = function(_, path, only_dirs, return_path)
        path = normalize(path)
        local result = _ITG_DIR_LISTING(path, only_dirs == true, return_path == true)
        local files = _ITG_DIR_LISTING(path, false, true)
        local query = { path = source_path(path), only_dirs = only_dirs == true, return_path = return_path == true, files = {} }
        for _, file in ipairs(files) do query.files[#query.files + 1] = source_path(file) end
        file_observations.directories[#file_observations.directories + 1] = query
        return result
    end }

SOUND = {
    GetPlayerBalance = function(_, player)
        if is_double then return 0 end
        return player_key(player) == "PLAYER_1" and -1 or 1
    end,
    PlayMusicPart = function(_, ...) emit("call", nil, "SoundManager.PlayMusicPart", safe_args(...)) end,
	PlayOnce = function(_, path) emit("call", nil, "SoundManager.PlayOnce", safe_args(path)) end,
	DimMusic = function(_, volume, seconds) emit("call", nil, "SoundManager.DimMusic", safe_args(volume, seconds)) end,
}
NOTESKIN = {
	LoadActorForNoteSkin = function(_, button, element) return LoadActorForNoteSkin(button, element) end,
	GetMetricFForNoteSkin = function() error("native noteskin resources unavailable", 0) end,
	GetMetricBForNoteSkin = function() error("native noteskin resources unavailable", 0) end,
}
if reference_skin then
	local function check_skin(skin)
		assert(skin == nil or tostring(skin):lower() == reference_skin.skin:lower(), "noteskin was not captured: " .. tostring(skin))
	end
	local function metric(section, key)
		local value = reference_skin.metrics[(tostring(section) .. "/" .. tostring(key)):lower()]
			or reference_skin.metrics[("NoteDisplay/" .. tostring(key)):lower()]
		assert(value, "native noteskin metric unavailable: " .. tostring(section) .. "/" .. tostring(key))
		return value
	end
	function NOTESKIN:GetPath(button, element)
		return assert(reference_skin.paths[(tostring(button) .. "/" .. tostring(element)):lower()],
			"native noteskin path unavailable: " .. tostring(button) .. "/" .. tostring(element))
	end
	function NOTESKIN:GetMetricFForNoteSkin(section, key, skin) check_skin(skin); return metric(section, key).number end
	function NOTESKIN:GetMetricBForNoteSkin(section, key, skin) check_skin(skin); return metric(section, key).boolean end
	function NOTESKIN:LoadActorForNoteSkin(button, element, skin) check_skin(skin); return self:LoadActor(button, element) end
	function NOTESKIN:GetMetricA(section, key)
		local raw = metric(section, key).raw
		if raw:sub(1, 1) == "%" then return assert(loadstring("return " .. raw:sub(2)))() end
		return assert(loadstring("return cmd(" .. raw .. ")"))()
	end
	function NOTESKIN:LoadActor(button, element)
		local path = self:GetPath(button, element)
		if path:lower():match("%.lua$") then
			local prior_var = Var
			Var = function(name)
				if name == "Button" then return button end
				if name == "Element" then return element end
				return prior_var and prior_var(name)
			end
			local ok, actor = pcall(execute_file, path)
			Var = prior_var
			if not ok then error(actor, 0) end
			return actor
		end
		return Def.Sprite { Texture = path }
	end
end

SCREEN_WIDTH, SCREEN_HEIGHT = harness.screen_width, harness.screen_height
SCREEN_CENTER_X, SCREEN_CENTER_Y = SCREEN_WIDTH / 2, SCREEN_HEIGHT / 2
SCREEN_LEFT, SCREEN_RIGHT, SCREEN_TOP, SCREEN_BOTTOM = 0, SCREEN_WIDTH, 0, SCREEN_HEIGHT
_screen = { w = SCREEN_WIDTH, h = SCREEN_HEIGHT, cx = SCREEN_CENTER_X, cy = SCREEN_CENTER_Y, left = SCREEN_LEFT, right = SCREEN_RIGHT, top = SCREEN_TOP, bottom = SCREEN_BOTTOM }
PLAYER_1, PLAYER_2 = "PlayerNumber_P1", "PlayerNumber_P2"
NUM_PLAYERS, NUM_COLS = 2, 4
Difficulty_Beginner, Difficulty_Easy, Difficulty_Medium = "Difficulty_Beginner", "Difficulty_Easy", "Difficulty_Medium"
Difficulty_Hard, Difficulty_Challenge, Difficulty_Edit = "Difficulty_Hard", "Difficulty_Challenge", "Difficulty_Edit"
ModsLevel_Preferred, ModsLevel_Stage, ModsLevel_Song, ModsLevel_Current = "ModsLevel_Preferred", "ModsLevel_Stage", "ModsLevel_Song", "ModsLevel_Current"
BlendMode_Normal, BlendMode_Add, BlendMode_Subtract = "BlendMode_Normal", "BlendMode_Add", "BlendMode_Subtract"
EffectClock_BGM_TIME, EffectClock_BGM_BEAT = "EffectClock_BGM_TIME", "EffectClock_BGM_BEAT"
left, center, right = "HorizAlign_Left", "HorizAlign_Center", "HorizAlign_Right"
top, middle, bottom = "VertAlign_Top", "VertAlign_Middle", "VertAlign_Bottom"
align_left, align_center, align_right, align_top, align_middle, align_bottom = 0, 0.5, 1, 0, 0.5, 1
PlayerOptions = _ITG_PLAYER_OPTION_METHODS
-- Feature probes must reflect the native class bindings. In particular,
-- ITGmania has neither SM5.2's NoteField.set_skin nor Player.SetNoteData.
NoteField = {
	set_step_callback = true, set_set_pressed_callback = true,
	set_did_tap_note_callback = true, set_did_hold_note_callback = true,
	step = true, set_pressed = true, did_tap_note = true, did_hold_note = true,
	get_column_actors = true, GetBeatBars = true, SetBeatBars = true, SetBeatBarsAlpha = true,
}
Player = {
	SetLife = true, ChangeLife = true,
	SetActorWithJudgmentPosition = true, SetActorWithComboPosition = true,
	GetPlayerTimingData = true, get_oitg_zoom_mode = true, set_oitg_zoom_mode = true,
}
Difficulty = {
	Difficulty_Beginner, Difficulty_Easy, Difficulty_Medium,
	Difficulty_Hard, Difficulty_Challenge, Difficulty_Edit,
	Reverse = function()
		return {
			[Difficulty_Beginner] = 0, [Difficulty_Easy] = 1,
			[Difficulty_Medium] = 2, [Difficulty_Hard] = 3,
			[Difficulty_Challenge] = 4, [Difficulty_Edit] = 5,
		}
	end,
}
function Year() return 2026 end
function MonthOfYear() return 9 end
function DayOfMonth() return 1 end
function DayOfYear() return 273 end
function Weekday() return 4 end
function Hour() return 12 end
function Minute() return 0 end
function Second() return 0 end
function PlayerColor() return { 1, 1, 1, 1 } end
function PlayerNumberToString(player) return player_key(player) == "PLAYER_2" and "P2" or "P1" end
function lerp_color(t, from, to)
	local out = {}
	for index = 1, 4 do out[index] = (from[index] or 0) + ((to[index] or 0) - (from[index] or 0)) * t end
	return out
end
math.round = math.round or round
print = function() end

local function base_ease(t, b, c, d)
	if d == nil or d == 0 then return t end
	return (b or 0) + (c or 1) * t / d
end
ease = setmetatable({ linear = base_ease }, { __index = function() return base_ease end })
mods, mods2, mod_actions = {}, {}, {}
local function schedule_beat(beat, action, params)
	scheduled_beats[#scheduled_beats + 1] = { beat = tonumber(beat) or 0, action = action, params = params }
end
function mod_message(beat, action, params) schedule_beat(beat, action, params) end
function mod_insert(beat, length, value, _, player) schedule_beat(beat, { mod = value, player = player, length = length }) end
function mod2_insert(beat, length, value, _, player) mod_insert(beat, length, value, nil, player) end
function mod_ease(beat, length, _, finish, name, _, _, player)
	mod_insert(beat, length, tostring(finish) .. " " .. tostring(name), nil, player)
end
function simple_m0d(beat, strength, _, name, player) mod_insert(beat, 0.1, tostring(strength or 400) .. " " .. tostring(name or "drunk"), nil, player) end
simple_m0d2, simple_m0d3 = simple_m0d, simple_m0d
function taronuke_mods(value, player)
	local key = tonumber(player) == 2 and "PLAYER_2" or "PLAYER_1"
	emit("modifier", player_options[key], "PlayerState.SetPlayerOptions", safe_args("ModsLevel_Song", tostring(value)))
end
modhelpers = setmetatable({ mod_message = mod_message, mod_insert = mod_insert, mod2_insert = mod2_insert, mod_ease = mod_ease }, { __index = function()
	return function() end
end })
ArrowEffects = setmetatable({}, { __index = function(_, name)
	if name == "GetRotationX" then
		return function(_, _, column)
			return tonumber(column) == 0 and harness.legacy_rotation_x or 0
		end
	end
	if name == "GetZoom" or name == "GetAlpha" then return function() return 1 end end
	if name == "GetXPos" then
		return function(_, column)
			column = tonumber(column) or 1
			return column >= 1 and column <= column_count and (column - (column_count + 1) / 2) * 64 or 0
		end
	end
	if name == "GetYPos" then return function(state, column, offset, reverse_offset)
		-- ArrowGetReverseShiftAndScale in the local ArrowEffects.cpp. Query
		-- the linked PlayerOptions and retain native float arithmetic.
		local options = player_options[state.player]
		local reverse = options:GetReversePercentForColumn((tonumber(column) or 1) - 1)
		local zoom = _ITG_FLOAT(1 - _ITG_FLOAT(_ITG_OPTIONS_UPDATE(options.native_index, "Mini", nil) * 0.5))
		if math.abs(zoom) < 0.01 then zoom = _ITG_FLOAT(0.01) end
		local half = _ITG_FLOAT(_ITG_FLOAT((tonumber(reverse_offset) or 270) / zoom) / 2)
		local shift = _ITG_FLOAT(_ITG_FLOAT(reverse * _ITG_FLOAT(half + half)) - half)
		shift = _ITG_FLOAT(_ITG_FLOAT(_ITG_OPTIONS_UPDATE(options.native_index, "Centered", nil) * -shift) + shift)
		local scale = _ITG_FLOAT(_ITG_FLOAT(reverse * -2) + 1)
		return _ITG_FLOAT(_ITG_FLOAT((tonumber(offset) or 0) * scale) + shift)
	end end
	if name == "GetYOffset" then return function(_, _, beat)
		beat = tonumber(beat) or 0
		if _ITG_TIMING_Y_OFFSET then return _ITG_TIMING_Y_OFFSET(beat, current_beat, current_seconds) end
		return 64 * (beat - current_beat)
	end end
	return function() return 0 end
end })
RageFileUtil = { CreateRageFile = function()
    local file = { handle = _ITG_FILE_CREATE() }
    return setmetatable(file, { __index = function(_, method) return function(self, ...)
        local args = {...}
        local value = _ITG_FILE_CALL(self.handle, method, ...)
        if method == "Open" then
            self.path, self.mode = source_path(normalize(args[1])), args[2]
            if self.mode % 2 == 1 then
                file_observations.reads[#file_observations.reads + 1] = { path = self.path, exists = value, generated = file_observations.generated[self.path] == true }
            elseif value then
                file_observations.generated[self.path] = true
                file_observations.writes[#file_observations.writes + 1] = { path=self.path, operation=method, mode=self.mode }
            end
        elseif method == "Write" or method == "PutLine" then
            file_observations.writes[#file_observations.writes + 1] = { path=self.path, operation=method, text=args[1], result=value }
        end
        return value
    end end })
end }

function GetTimeSinceStart() return current_seconds end
_G.type = function(value)
    if type(value) == "table" and getmetatable(value) == actor_mt then return "userdata" end
    return type(value)
end
ThemeManager, GameState = THEME, GAMESTATE
for _, helper in ipairs(_ITG_THEME_HELPERS or {}) do
    assert(loadstring(helper.source, "@theme:/_fallback/Scripts/" .. helper.name))()
end
ThemePrefs.Init({}, true)

local function run_scheduled_beats()
	local index = 1
	while index <= #scheduled_beats do
		local item = scheduled_beats[index]
		if item.beat <= current_beat + 0.000001 then
			table.remove(scheduled_beats, index)
			if type(item.action) == "function" then
				local source, line = command_info(item.action)
				local prior = active_context
				active_context = { callback = "scheduled_beat", source = source, line = line }
				local ok, message = pcall(item.action, item.params)
				if not ok then record_runtime_error(nil, "callback.error", "scheduled_beat", message) end
				active_context = prior
			elseif type(item.action) == "string" then
				broadcast(item.action, item.params)
			elseif type(item.action) == "table" and item.action.mod then
				local key = tonumber(item.action.player) == 2 and "PLAYER_2" or "PLAYER_1"
				emit("modifier", player_options[key], "PlayerState.SetPlayerOptions", safe_args("ModsLevel_Song", tostring(item.action.mod)))
			end
		else
			index = index + 1
		end
	end
end

local function run_callback(actor, kind, fn, delta)
	local source, line = command_info(fn)
	local prior = active_context
	active_context = { definition_id = actor.definition_id, callback = kind, source = source, line = line }
	local ok, message = pcall(fn, actor, delta)
	if not ok then
		record_runtime_error(actor, "callback.error", kind, message)
		if kind == "SetUpdateFunction" then actor.update_fn = nil end
		if kind == "SetDrawFunction" then actor.draw_fn = nil end
	end
	active_context = prior
end

function manual.models.update(model, delta)
	if not model then return end
	local ok, message = _ITG_MODEL_UPDATE(model, delta)
	if not ok then error(message, 0) end
end

local function advance_actor(actor, delta)
	local model_delta = delta
	if (rawget(actor, "hibernate_seconds") or 0) > 0 then
		actor.hibernate_seconds, delta = _ITG_HIBERNATE_STEP(actor.hibernate_seconds, delta)
		if delta == nil then
			-- Model::Update still advances its materials when its Actor::Update
			-- returns early. A sleeping parent never calls this child update.
			manual.models.update(rawget(actor, "native_model"), model_delta)
			return
		end
	end
	for _, wrapper in ipairs(actor.wrappers or {}) do advance_actor(wrapper, delta) end
	-- ActorFrame::UpdateInternal multiplies its delta after Actor::Update has
	-- woken the owner and updated its wrappers, before its tweens and children.
	delta = _ITG_FLOAT(delta * (rawget(actor, "update_rate") or 1))
	-- Actor::UpdateInternal advances the effect clock, accumulates spin into
	-- current rotation, then interpolates the complete queued tween state.
	local clock = tostring(actor.state.effectclock or "timer"):lower()
	local previous = actor.state.spinclock or 0
	local units, elapsed
	if clock == "timer" then
		elapsed = _ITG_FLOAT(delta)
		units = _ITG_FLOAT(previous + elapsed)
		local period = tonumber(actor.state.effectperiod) or 1
		if units > period then units = _ITG_FLOAT(units - period) end
	else
		-- GameState::UpdateSongPosition passes raw/visible music timestamps
		-- to Actor::SetBGMTime, independently of the elapsed update delta.
		if clock == "music" then units = song_position:GetMusicSecondsVisible()
		elseif clock == "musicnooffset" then units = song_position:GetMusicSeconds()
		else units = _ITG_FLOAT((clock == "bgm" or clock:find("beat", 1, true)) and current_beat or current_seconds) end
		elapsed = _ITG_FLOAT(units - previous)
	end
	actor.state.spinclock = units
	actor.effect_delta = elapsed
	if actor.state.effect == "spin" then
		local magnitude = actor.state.effectmagnitude or { 0, 0, 180 }
		for index, axis in ipairs({ "x", "y", "z" }) do
			local key = "rotation" .. axis
			actor.state[key] = _ITG_SPIN_ROTATION(tonumber(actor.state[key]) or 0, tonumber(magnitude[index]) or 0, elapsed)
		end
	end
	advance_tween(actor, delta)
	-- Native Model::Update runs Actor commands before bones and materials,
	-- retaining the incoming delta even when its own hibernation shortens it.
	manual.models.update(rawget(actor, "native_model"), model_delta)
	for _, child in ipairs(actor.children or {}) do advance_actor(child, delta) end
	-- ActorFrame::UpdateInternal calls its callback after its own children,
	-- before later siblings consume this frame's delta.
	local update_fn = rawget(actor, "update_fn")
	if type(update_fn) == "function" then run_callback(actor, "SetUpdateFunction", update_fn, delta) end
end

-- Bind the linked RageMath and RageDisplay float path. Lua double arithmetic
-- loses native rounding near the camera plane, magnifying tiny differences.
local function mat_identity()
	return { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1 }
end

local function mat_mul(left, right)
	return _ITG_MATRIX_MUL(left, right)
end

local function mat_translate(x, y, z)
	local out = mat_identity()
	out[13], out[14], out[15] = x, y, z
	return out
end

local function mat_scale(x, y, z)
	local out = mat_identity()
	out[1], out[6], out[11] = x, y, z
	return out
end

local function mat_skew_x(amount)
	local out = mat_identity()
	out[5] = amount
	return out
end

local function mat_skew_y(amount)
	local out = mat_identity()
	out[2] = amount
	return out
end

local function mat_rotation_xyz(x, y, z)
	return _ITG_ROTATION_MATRIX(x, y, z)
end

local function vec_transform(vector, matrix)
	return _ITG_VECTOR_TRANSFORM(vector, matrix)
end

local function actor_rotation(actor, axis)
	return tonumber(actor.state["rotation" .. axis]) or 0
end

local function actor_local_matrix(actor, width, height, parent_world)
	local state = actor.state
	local zoom = tonumber(state.zoom) or 1
	local axes = { tonumber(state.zoomx) or zoom, tonumber(state.zoomy) or zoom, tonumber(state.zoomz) or zoom }
	local position = { tonumber(state.x) or 0, tonumber(state.y) or 0, tonumber(state.z) or 0 }
	local rotation = { actor_rotation(actor, "x"), actor_rotation(actor, "y"), actor_rotation(actor, "z") }
	if state.effect == "bob" or state.effect == "bounce" or state.effect == "wag" then
		local period = tonumber(state.effectperiod) or 2
		position, rotation = _ITG_MOTION_POSE(state.effect, tonumber(state.spinclock) or 0,
			position, rotation, state.effectmagnitude or { 0, 20, 0 },
			state.effecttiming or { period / 2, 0, period / 2, 0, 0 }, tonumber(state.effectoffset) or 0)
	end
	if state.effect == "pulse" then
		axes = _ITG_PULSE_ZOOM(tonumber(state.spinclock) or 0, axes,
			state.effectmagnitude or { 0.5, 1, 10 }, state.effectcolor1 or { 1, 1, 1, 1 },
			state.effectcolor2 or { 1, 1, 1, 1 }, state.effecttiming or { 1, 0, 1, 0, 0 },
			tonumber(state.effectoffset) or 0)
	end
	-- Actor::BeginDraw adds base rotation to the effect-applied tween rotation.
	for index, axis in ipairs({"x", "y", "z"}) do
		rotation[index] = _ITG_FLOAT(rotation[index] + (tonumber(state["baserotation" .. axis]) or 0))
	end
	local sx = _ITG_FLOAT(axes[1] * (tonumber(state.basezoomx) or 1))
	local sy = _ITG_FLOAT(axes[2] * (tonumber(state.basezoomy) or 1))
	local sz = _ITG_FLOAT(axes[3] * (tonumber(state.basezoomz) or 1))
	local align_x = (0.5 - (tonumber(state.halign) or 0.5)) * width
	local align_y = (0.5 - (tonumber(state.valign) or 0.5)) * height
	local matrix = mat_mul(mat_translate(unpack(position)), parent_world)
	matrix = mat_mul(mat_rotation_xyz(unpack(rotation)), matrix)
	matrix = mat_mul(mat_scale(sx, sy, sz), matrix)
	if align_x ~= 0 or align_y ~= 0 then matrix = mat_mul(mat_translate(align_x, align_y, 0), matrix) end
	matrix = mat_mul(mat_skew_x(tonumber(state.skewx) or 0), matrix)
	matrix = mat_mul(mat_skew_y(tonumber(state.skewy) or 0), matrix)
	return matrix
end

local function actor_draw_matrix(actor, width, height, inherited)
	local wrappers = rawget(actor, "wrappers") or {}
	for index = #wrappers, 1, -1 do
		inherited = actor_draw_matrix(wrappers[index], 0, 0, inherited)
	end
	return actor_local_matrix(actor, width, height, inherited)
end

local world_matrix_cache, projection_cache, color_cache = {}, {}, {}
local function actor_world_matrix(actor, width, height)
	if width == 0 and height == 0 and world_matrix_cache[actor] then return world_matrix_cache[actor] end
	local parent = rawget(actor, "parent")
	-- RageTextureRenderTarget::BeginRenderingTo resets the world stack.
	local parent_world = parent and parent.class ~= "ActorFrameTexture"
		and actor_world_matrix(parent, 0, 0) or mat_identity()
	-- Actor::BeginDraw applies each transform to the inherited world stack.
	-- Grouping the child's matrices first changes float rounding near W=0.
	local world = actor_draw_matrix(actor, width, height, parent_world)
	if width == 0 and height == 0 then world_matrix_cache[actor] = world end
	return world
end

local function perspective_ancestor(actor)
	local current = actor.parent
	while current do
		if current.class == "ActorFrameTexture" then return nil end
		if current.state.fov ~= nil and tonumber(current.state.fov) ~= -1 then return current end
		current = rawget(current, "parent")
	end
	return nil
end

local function actor_viewport(actor)
	local current = rawget(actor, "parent")
	while current do
		if current.class == "ActorFrameTexture" then
            local texture = rawget(current, "allocated_texture")
            return texture and texture.state.sourcewidth or harness.screen_width,
                texture and texture.state.sourceheight or harness.screen_height
		end
		current = rawget(current, "parent")
	end
	return harness.screen_width, harness.screen_height
end

-- Headless Lua worker, single-threaded, one-frame lifetime. Cache at most one
-- entry per sampled actor/ancestor; clear before each geometry sampling pass.
local function actor_draw_colors(actor, diffuse, glow)
    local wrappers = rawget(actor, "wrappers") or {}
    for index = #wrappers, 1, -1 do
        diffuse, glow = actor_draw_colors(wrappers[index], diffuse, glow)
    end
    local state = actor.state
    local mode = state.effect
    if not COLOR_EFFECTS[mode] and mode ~= "rainbow" then mode = "none" end
    local period = tonumber(state.effectperiod) or 1
    return _ITG_ACTOR_COLORS(mode, tonumber(state.spinclock) or 0,
        state_value(state, "diffuse"), state_value(state, "glow"),
        state.effectcolor1 or {1,1,1,1}, state.effectcolor2 or {1,1,1,1},
        state.effecttiming or {period/2,0,period/2,0,0}, tonumber(state.effectoffset) or 0,
        diffuse, glow)
end

local function actor_colors(actor)
    if color_cache[actor] then return unpack(color_cache[actor]) end
    local parent = rawget(actor, "parent")
    local diffuse, glow = {1,1,1,1}, {0,0,0,0}
    if parent then diffuse, glow = actor_colors(parent) end
    diffuse, glow = actor_draw_colors(actor, diffuse, glow)
    color_cache[actor] = {diffuse, glow}
    return diffuse, glow
end

local function actor_draw_visible(actor)
    if actor.state.visible == false then return false end
    if (rawget(actor, "hibernate_seconds") or 0) > 0 then return false end
    for _, wrapper in ipairs(rawget(actor, "wrappers") or {}) do
        if not actor_draw_visible(wrapper) then return false end
    end
    return true
end

local function actor_visibility(actor)
    local visible, current = true, actor
    while current do
        visible = visible and actor_draw_visible(current)
        if current.class == "ActorFrameTexture" and not rawget(current, "allocated_texture") then visible = false end
        current = rawget(current, "parent")
    end
    local diffuse, glow = actor_colors(actor)
    return visible, diffuse[4], diffuse, glow
end

local function menu_projection(camera)
	if projection_cache[camera] then
		local cached = projection_cache[camera]
		return unpack(cached)
	end
	local width, height = harness.screen_width, harness.screen_height
	local fov = tonumber(camera.state.fov) or 0
	-- RageDisplay::LoadMenuPerspective selects orthographic matrices for zero
	-- before clamping nonzero FOV. Preserve that branch in the native call.
	if fov ~= 0 then fov = math.max(0.1, math.min(179.9, fov)) end
	local vanish = camera.state.vanishpoint or { width / 2, height / 2 }
	local vanish_x, vanish_y = tonumber(vanish[1]) or width / 2, tonumber(vanish[2]) or height / 2
	local view, projection, dist = _ITG_MENU_MATRICES(width, height, fov, vanish_x, vanish_y)
	local result = { view, projection, fov, vanish_x, vanish_y, dist }
	projection_cache[camera] = result
	return unpack(result)
end

local image_size_cache = {}
local function be32(data, offset)
	local a, b, c, d = data:byte(offset, offset + 3)
	if not d then return nil end
	return ((a * 256 + b) * 256 + c) * 256 + d
end

texture_path = function(actor)
	local texture = actor_texture(actor)
	if type(texture) == "table" and texture.class == "RageTexture" then return texture.state.path end
	if type(texture) ~= "string" or texture == "" then return nil end
	texture = normalize(texture)
	local candidate
	if texture:sub(1, 6) == "song:/" then candidate = join_path(song_dir, texture:sub(7))
	elseif absolute(texture) then candidate = texture
	else
		local source = normalize(actor.source or "")
		local source_dir = song_dir
		if source:sub(1, 6) == "song:/" then source_dir = dirname(join_path(song_dir, source:sub(7))) end
		candidate = join_path(source_dir, texture)
	end
	if is_file(candidate) then return candidate end
	local without_extension = candidate:gsub("%.[^./]+$", "")
	local resolved = texture_aliases[without_extension:lower()]
	if resolved then return resolved end
	return candidate
end

-- ISO BMFF track headers carry the movie's native source dimensions. Read only
-- container headers and the eight dimension bytes, never the video payload.
local function movie_size(file, begin, limit, depth)
    if depth > 4 then return nil end
    local pos = begin
    while pos + 8 <= limit do
        file:seek("set", pos)
        local header = file:read(8) or ""
        local size, kind = be32(header, 1), header:sub(5, 8)
        if not size then return nil end
        local payload = pos + 8
        if size == 1 then
            local extended = file:read(8) or ""
            local high, low = be32(extended, 1), be32(extended, 5)
            if not high or not low then return nil end
            size, payload = high * 4294967296 + low, pos + 16
        elseif size == 0 then size = limit - pos end
        local finish = pos + size
        if finish <= payload or finish > limit then return nil end
        if kind == "tkhd" and size >= 84 then
            file:seek("set", finish - 8)
            local dims = file:read(8) or ""
            local width, height = be32(dims, 1), be32(dims, 5)
            if width and height and width > 0 and height > 0 then return width / 65536, height / 65536 end
        elseif kind == "moov" or kind == "trak" then
            local width, height = movie_size(file, payload, finish, depth + 1)
            if width and height then return width, height end
        end
        pos = finish
    end
end

local function le32(data, offset)
    local a, b, c, d = data:byte(offset, offset + 3)
    if not d then return nil end
    return ((d * 256 + c) * 256 + b) * 256 + a
end

-- AVI's video stream format owns the decoded dimensions. Skip movi payloads,
-- audio formats and the main header's potentially stale width and height.
local function avi_size(file, begin, limit, depth)
    if depth > 4 then return nil end
    local pos, video = begin, false
    while pos + 8 <= limit do
        file:seek("set", pos)
        local header = file:read(8) or ""
        local kind, size = header:sub(1, 4), le32(header, 5)
        if not size then return nil end
        local payload, finish = pos + 8, pos + 8 + size
        if finish > limit then return nil end
        if kind == "LIST" and size >= 4 then
            local list = file:read(4)
            if list == "hdrl" or list == "strl" then
                local width, height = avi_size(file, payload + 4, finish, depth + 1)
                if width and height then return width, height end
            end
        elseif kind == "strh" and size >= 4 then
            video = file:read(4) == "vids"
        elseif kind == "strf" and video and size >= 40 then
            local format = file:read(12) or ""
            local length, width, height = le32(format, 1), le32(format, 5), le32(format, 9)
            if height and height >= 2147483648 then height = height - 4294967296 end
            if length and length >= 40 and length <= size and width and width > 0
                and width < 2147483648 and height and height ~= 0 then
                return width, math.abs(height)
            end
        end
        pos = finish + size % 2
    end
end

image_size = function(path)
	if image_size_cache[path] ~= nil then return image_size_cache[path][1], image_size_cache[path][2] end
	local file = io.open(path, "rb")
	if not file then image_size_cache[path] = {}; return nil end
    if path:lower():match("%.jpe?g$") then
        file:close()
        local info = _ITG_TEXTURE_INFO(path)
        local width, height = info.sourceframewidth, info.sourceframeheight
        image_size_cache[path] = { width, height }
        return width, height
    end
    if path:lower():match("%.avi$") then
        local limit = file:seek("end")
        file:seek("set", 0)
        local header = file:read(12) or ""
        local size = le32(header, 5)
        local width, height
        if header:sub(1, 4) == "RIFF" and header:sub(9, 12) == "AVI "
            and size and size >= 4 and size + 8 <= limit then
            width, height = avi_size(file, 12, size + 8, 0)
        end
        file:close()
        image_size_cache[path] = { width, height }
        return width, height
    end
    if path:lower():match("%.m[p4o][4v]$") then
        local width, height = movie_size(file, 0, file:seek("end"), 0)
        file:close()
        image_size_cache[path] = { width, height }
        return width, height
    end
	local header = file:read(24) or ""
	file:close()
	if header:sub(1, 8) ~= "\137PNG\r\n\26\n" then image_size_cache[path] = {}; return nil end
	local width, height = be32(header, 17), be32(header, 21)
	-- RageTexture::GetFrameDimensionsFromFileName: Sprite geometry uses one frame.
	local cols, rows = (path .. " "):match(" (%d+)x(%d+)[. ]")
	cols, rows = tonumber(cols), tonumber(rows)
	if cols and rows and cols > 0 and rows > 0 and cols <= width and rows <= height then
		width, height = width / cols, height / rows
	end
	image_size_cache[path] = { width, height }
	return width, height
end

-- Texture metadata belongs to the source, not the Sprite borrowing it. An old
-- GetTexture handle remains valid when its owner loads a different image.
local image_textures = {}
image_texture = function(path)
	if image_textures[path] then return image_textures[path] end
	local texture = external_actor("Texture/" .. path, "RageTexture")
	local width, height = image_size(path)
	width, height = width or 256, height or 256
	if (path:lower():match("%.png$") or path:lower():match("%.jpe?g$")) and is_file(path) then
		texture.state = _ITG_TEXTURE_INFO(path)
	else
		texture.state = {
			sourcewidth = width, sourceheight = height,
			sourceframewidth = width, sourceframeheight = height,
			texturewidth = width, textureheight = height,
			imagewidth = width, imageheight = height, numframes = 1,
		}
	end
	texture.state.path = path
	image_textures[path] = texture
	return texture
end

local function same_sample(left, right)
	if left == nil or #left ~= #right then return false end
	for index = 1, #left do if left[index] ~= right[index] then return false end end
	return true
end

local function actor_effect_chain(actor)
    local effects, current = {}, actor
    local function append_effect(state)
        local effect = state.state.effect
        if effect == nil then return end
        local magnitude = state.state.effectmagnitude or { 0, 0, 0 }
        effects[#effects + 1] = {
            actor = state.id, mode = effect,
            magnitude = {
                tonumber(magnitude[1]) or 0,
                tonumber(magnitude[2]) or 0,
                tonumber(magnitude[3]) or 0,
            },
        }
    end
    while current do
        if current.class == "ActorFrameTexture" then break end
        append_effect(current)
        -- Actor::Draw applies direct wrappers with PreDraw/BeginDraw, highest
        -- index first. This leaf-to-root chain records that stack in reverse;
        -- a wrapper's own wrappers are not entered by BeginDraw.
        for _, wrapper in ipairs(rawget(current, "wrappers") or {}) do
            append_effect(wrapper)
        end
        current = rawget(current, "parent")
    end
    return effects
end

local function sample_signature(visible, alpha, vertices, camera, effects, diffuse, glow)
	if not visible or (alpha <= 0.000001 and glow[4] <= 0.000001) then return { 0, alpha } end
	local values = { 1, alpha }
	for _, vertex in ipairs(vertices) do
		values[#values + 1] = vertex[1]
		values[#values + 1] = vertex[2]
		values[#values + 1] = vertex[3] or 0
	end
	for _, value in ipairs(camera) do values[#values + 1] = value end
	for _, effect in ipairs(effects) do
		values[#values + 1] = effect.actor
		values[#values + 1] = effect.mode
		for _, value in ipairs(effect.magnitude) do values[#values + 1] = value end
	end
	for _, color in ipairs({diffuse, glow}) do
		for _, value in ipairs(color) do values[#values + 1] = value end
	end
	return values
end

local function record_projected_actor(actor)
	if actor.class ~= "Sprite" and actor.class ~= "Quad" then return end
	local camera = perspective_ancestor(actor)
	local viewport_width, viewport_height = actor_viewport(actor)
	local visible, alpha, diffuse, glow = actor_visibility(actor)
	local track = projected_track_by_actor[actor]
	local effects = actor_effect_chain(actor)
	local crop = {}
	for _, key in ipairs({ "cropleft", "cropright", "croptop", "cropbottom" }) do
		crop[#crop + 1] = state_value(actor.state, key)
	end
	local shadow = { tonumber(actor.state.shadowlengthx) or 0, tonumber(actor.state.shadowlengthy) or 0 }
	for _, value in ipairs(actor.state.shadowcolor or {0, 0, 0, 0.5}) do shadow[#shadow + 1] = value end
	local path, width, height
	if actor.class == "Sprite" then
		local texture = actor_texture(actor)
		local owner = type(texture) == "table" and rawget(texture, "parent")
		if type(texture) == "table" and texture.class == "RageTexture"
			and owner and owner.class == "ActorFrameTexture" then
			path = "aft:" .. owner.id
			width, height = texture.state.sourceframewidth, texture.state.sourceframeheight
		else
			path = texture_path(actor)
			if not path then return end
			width, height = image_size(path)
		end
		if not width or not height then return end
	else
		path = ""
		width, height = tonumber(actor.state.width) or 1, tonumber(actor.state.height) or 1
	end
	width = tonumber(actor.state.width) or width
	height = tonumber(actor.state.height) or height
	if not track then
		track = {
			actor = actor.id, definition_id = actor.definition_id, class = actor.class,
			texture = source_path(path), texture_size = { width, height },
			camera_actor = camera and camera.id or "orthographic-screen",
			sample_layout = { "beat", "seconds", "visible", "alpha", "world_vertices", "clip_vertices", "screen_vertices", "camera", "effect_chain", "diffuse", "glow", "crop", "shadow" },
			samples = {}, texture_samples = {},
		}
		projected_track_by_actor[actor] = track
		projected_vertex_tracks[#projected_vertex_tracks + 1] = track
	end
	if actor.class == "Sprite" then
		local previous = track.texture_samples[#track.texture_samples]
		local source = source_path(path)
		if not previous or previous[3] ~= source or previous[4] ~= width or previous[5] ~= height then
			track.texture_samples[#track.texture_samples + 1] = { current_beat, current_seconds, source, width, height }
		end
	end
	if not visible or (alpha <= 0.000001 and glow[4] <= 0.000001) then
		local camera_sample = camera and { tonumber(camera.state.fov) or 0, 0, 0, 0 }
			or { 0, viewport_width / 2, viewport_height / 2, 0 }
		local signature = sample_signature(false, alpha, {}, camera_sample, effects, diffuse, glow)
		for _, value in ipairs(crop) do signature[#signature + 1] = value end
		for _, value in ipairs(shadow) do signature[#signature + 1] = value end
		if same_sample(projected_signature_by_actor[actor], signature) then return end
		projected_signature_by_actor[actor] = signature
		track.samples[#track.samples + 1] = { current_beat, current_seconds, false, safe_value(alpha, 0, {}), {}, {}, {}, camera_sample, effects, safe_value(diffuse, 0, {}), safe_value(glow, 0, {}), crop, shadow }
		return
	end
	local world = actor_world_matrix(actor, width, height)
	local view, projection, fov, vanish_x, vanish_y, distance
	if camera then view, projection, fov, vanish_x, vanish_y, distance = menu_projection(camera) end
	local local_vertices = {
		{ -width / 2, -height / 2, 0, 1 }, { width / 2, -height / 2, 0, 1 },
		{ width / 2, height / 2, 0, 1 }, { -width / 2, height / 2, 0, 1 },
	}
	local world_vertices, clip_vertices, screen_vertices = {}, {}, {}
	for index, vertex in ipairs(local_vertices) do
		local world_vertex = vec_transform(vertex, world)
		world_vertices[index] = { world_vertex[1], world_vertex[2], world_vertex[3] }
		if camera then
			local clip = vec_transform(vec_transform(world_vertex, view), projection)
			clip_vertices[index] = { clip[1], clip[2], clip[3], clip[4] }
			if clip[4] == 0 then return end
			screen_vertices[index] = _ITG_SCREEN_VERTEX(clip, viewport_width, viewport_height)
		else
			clip_vertices[index] = {
				world_vertex[1] * 2 / viewport_width - 1,
				1 - world_vertex[2] * 2 / viewport_height,
				world_vertex[3], 1,
			}
			screen_vertices[index] = { world_vertex[1], world_vertex[2] }
		end
	end
	local camera_sample = camera and { fov, vanish_x, vanish_y, distance }
		or { 0, viewport_width / 2, viewport_height / 2, 0 }
	local signature = sample_signature(visible, alpha, screen_vertices, camera_sample, effects, diffuse, glow)
	for _, value in ipairs(crop) do signature[#signature + 1] = value end
	for _, value in ipairs(shadow) do signature[#signature + 1] = value end
	if same_sample(projected_signature_by_actor[actor], signature) then return end
	projected_signature_by_actor[actor] = signature
	track.samples[#track.samples + 1] = {
		current_beat, current_seconds, visible, safe_value(alpha, 0, {}), world_vertices, clip_vertices, screen_vertices,
		camera_sample, effects, safe_value(diffuse, 0, {}), safe_value(glow, 0, {}), crop, shadow,
	}
end

-- The isolated capture's single Lua thread owns this index for one simfile.
-- Its exact serialized keys share immutable observations, never approximate
-- float values. Index capacity: 65,536 keys and 64 MiB of key bytes. On
-- saturation, retain every new buffer without indexing it; existing hits
-- still share. No pruning occurs in the frame loop. The output buffers are
-- reference data, bounded by the complete capture's frames and mesh sizes,
-- and are released with the session. Counters expose hits and saturation.
function manual.models.intern(values)
	local parts = {tostring(#values), ":"}
	for _, row in ipairs(values) do
		parts[#parts+1] = tostring(#row) .. ":"
		for _, value in ipairs(row) do
			assert(type(value) == "number" or value == _ITG_JSON_NULL, "non-numeric native Model column")
			parts[#parts+1] = type(value) == "number" and finite(value)
				and string.format("%.17g", value) or "null"
			parts[#parts+1] = ","
		end
		parts[#parts+1] = ";"
	end
	local key = table.concat(parts)
	local prior = manual.models.buffer_lookup[key]
	if prior then
		manual.models.buffer_hits = manual.models.buffer_hits + 1
		return prior
	end
	local id = #manual.models.buffers + 1
	manual.models.buffers[id] = values
	if manual.models.lookup_count < 65536 and manual.models.lookup_bytes + #key <= 67108864 then
		manual.models.buffer_lookup[key] = id
		manual.models.lookup_count = manual.models.lookup_count + 1
		manual.models.lookup_bytes = manual.models.lookup_bytes + #key
	else
		manual.models.saturated_misses = manual.models.saturated_misses + 1
	end
	return id
end

function manual.models.pack(primitives)
	for _, primitive in ipairs(primitives) do
		primitive.vertex_count = #primitive.vertices
		primitive.vertex_buffers = {}
		for _, field in ipairs(manual.models.vertex_fields) do
			local column = {}
			for index, vertex in ipairs(primitive.vertices) do column[index] = assert(vertex[field]) end
			primitive.vertex_buffers[field] = manual.models.intern(column)
		end
		primitive.normals_buffer = manual.models.intern(primitive.normals)
		primitive.texture_matrix_scale_buffer = manual.models.intern(primitive.texture_matrix_scale)
		primitive.vertices, primitive.normals, primitive.texture_matrix_scale = nil, nil, nil
	end
	return primitives
end

function manual.models.primitives(actor, world, view, projection, width, height, diffuse, glow)
	local primitives, message = _ITG_MODEL_DRAW(actor.native_model, diffuse, glow)
	if not primitives then error(message, 0) end
	if not view then view, projection = _ITG_MENU_MATRICES(width, height, 0, width/2, height/2) end
	for _, primitive in ipairs(primitives) do
		primitive.viewport = {width, height}
		if type(primitive.texture) == "string" then primitive.texture = source_path(primitive.texture) end
		for _, vertex in ipairs(primitive.vertices) do
			-- Native Model::DrawPrimitives has already flipped Y and applied bones.
			local position = vec_transform(vertex.world, world)
			vertex.world = position
			vertex.view = vec_transform(position, view)
			vertex.clip = vec_transform(vertex.view, projection)
			local inverse_w = vertex.clip[4] == 0 and 0 or _ITG_FLOAT(1/vertex.clip[4])
			vertex.ndc = {_ITG_FLOAT(vertex.clip[1]*inverse_w), _ITG_FLOAT(vertex.clip[2]*inverse_w),
				_ITG_FLOAT(vertex.clip[3]*inverse_w)}
			vertex.screen = _ITG_SCREEN_VERTEX(vertex.clip, width, height)
			vertex.screen[3] = vertex.ndc[3]
		end
	end
	return manual.models.pack(primitives)
end

function manual.models.record(actor)
	if actor.class ~= "Model" then return end
	local track = manual.models.by_actor[actor]
	if not track then
		track = {actor=actor.id, definition_id=actor.definition_id, class="Model", native_loaded=true,
			sample_layout={"beat","seconds","visible","primitives"}, samples={}}
		manual.models.by_actor[actor] = track
		manual.models.tracks[#manual.models.tracks + 1] = track
	end
	local visible, alpha, diffuse, glow = actor_visibility(actor)
	local primitives = {}
	if visible then
		local camera = perspective_ancestor(actor)
		local width, height = actor_viewport(actor)
		local view, projection
		if camera then view, projection = menu_projection(camera) end
		primitives = manual.models.primitives(actor, actor_world_matrix(actor, 1, 1), view, projection,
			width, height, diffuse, glow)
	end
	track.samples[#track.samples+1] = {current_beat,current_seconds,visible,primitives}
end

local function record_projected_vertices(loaded_roots)
	world_matrix_cache, projection_cache, color_cache = {}, {}, {}
	for _, root in ipairs(loaded_roots) do visit(root.actor, record_projected_actor) end
end

-- Explicit Draw uses the current draw stack, not the actor's tree parent.
-- Render-target Begin/Finish pushes and restores that stack; each Draw keeps
-- its own immutable pose/vertices so repeated color passes cannot collapse.
manual.begin = function(texture, preserve)
	manual.stack[#manual.stack + 1] = manual.context
	local width, height = texture.state.sourceframewidth, texture.state.sourceframeheight
	manual.context = { world = mat_identity(), width = width, height = height, target = texture.parent.id }
	manual.calls[#manual.calls + 1] = { operation = "begin", target = texture.parent.id,
		preserve = not not preserve, logical_size = {width, height},
		backing_size = {texture.state.texturewidth, texture.state.textureheight},
        alpha = texture.state.alpha == true,
        depth = texture.state.depth == true,
        float = texture.state.float == true }
end

manual.finish = function(texture)
	if not manual.stack[#manual.stack] then error("FinishRenderingTo without BeginRenderingTo") end
	manual.calls[#manual.calls + 1] = { operation = "finish", target = texture.parent.id }
	manual.context = table.remove(manual.stack)
end

manual.draw = function(actor)
    if not actor_draw_visible(actor) then return end
    local target = actor.class == "ActorFrameTexture" and rawget(actor, "allocated_texture")
    if actor.class == "ActorFrameTexture" and not target then return end
	local prior = manual.context
	local width, height = actor_size(actor)
	local world = actor_draw_matrix(actor, width, height, prior.world)
	local view, projection = prior.view, prior.projection
	if actor.state.fov ~= nil and tonumber(actor.state.fov) ~= -1 then view, projection = menu_projection(actor) end
	manual.context = { world = world, view = view, projection = projection,
		width = prior.width, height = prior.height, target = prior.target }
    if target then manual.begin(target, actor.state.enablepreservetexture) end
    if type(rawget(actor, "draw_fn")) == "function" then
        run_callback(actor, "SetDrawFunction", rawget(actor, "draw_fn"), 0)
    elseif (actor.class == "ActorFrame" or target) and rawget(actor, "definition_id") then
		for _, child in ipairs(actor.children or {}) do manual.draw(child) end
	else
		local diffuse, glow = actor_draw_colors(actor, {1,1,1,1}, {0,0,0,0})
		local call = { operation = "draw", actor = actor.id, path = actor.path, definition_id = rawget(actor, "definition_id"),
			class = actor.class, target = prior.target or "screen", state = safe_value(actor.state, 0, {}),
			world = world, diffuse = diffuse, glow = glow }
		local texture = actor_texture(actor)
		if type(texture) == "table" and texture.class == "RageTexture" then
			call.texture = texture.parent and texture.parent.class == "ActorFrameTexture"
				and "aft:" .. texture.parent.id or source_path(texture.state.path)
		elseif type(texture) == "string" then call.texture = source_path(texture_path(actor) or texture) end
		-- Resource handles remain identities; avoid serializing mutable actor trees.
		call.state.texture = nil
		if actor.class == "Model" then
			call.primitives = manual.models.primitives(actor, world, view, projection, prior.width, prior.height, diffuse, glow)
		elseif actor.class == "ActorMultiVertex" then
			call.primitives = _ITG_AMV_DRAW(actor.state.vertices or {}, actor.state.drawstate or {},
				tonumber(actor.state.linewidth) or 1, diffuse, glow)
			for _, primitive in ipairs(call.primitives) do
				for _, vertex in ipairs(primitive.vertices) do
					local position = vec_transform(vertex["local"], world)
					vertex.world = position
					if view then
						vertex.clip = vec_transform(vec_transform(position, view), projection)
						vertex.screen = _ITG_SCREEN_VERTEX(vertex.clip, prior.width, prior.height)
					else vertex.screen = {position[1], position[2]} end
				end
			end
		end
		manual.calls[#manual.calls + 1] = call
	end
    if target then manual.finish(target) end
    manual.context = prior
end

manual.run = function(loaded_roots)
	if not next(manual.actors) then return end
	world_matrix_cache, projection_cache, color_cache = {}, {}, {}
	manual.calls = {}
    local function visit_draw(actor)
        if not actor_draw_visible(actor) then return end
        if actor.class == "ActorFrameTexture" and not rawget(actor, "allocated_texture") then return end
		if type(rawget(actor, "draw_fn")) == "function" then
			local parent = actor.parent
			local camera = perspective_ancestor(actor)
			local view, projection
			if camera then view, projection = menu_projection(camera) end
			manual.context = { world = parent and actor_world_matrix(parent, 0, 0) or mat_identity(),
				view = view, projection = projection, width = harness.screen_width, height = harness.screen_height }
			manual.draw(actor)
			manual.context = nil
		else
			for _, child in ipairs(actor.children or {}) do visit_draw(child) end
		end
	end
	for _, root in ipairs(loaded_roots) do visit_draw(root.actor) end
	manual.frames[#manual.frames + 1] = { current_beat, current_seconds, manual.calls }
end

local function record_player_render(player_index, player)
	local source_visible = actor_visibility(player)
	local proxy_visible, proxies = false, {}
	for actor in pairs(player_proxy_sets[player_index]) do
		proxies[#proxies + 1] = actor.id
		local visible, alpha = actor_visibility(actor)
		proxy_visible = proxy_visible or (visible and alpha > 0.000001)
	end
	table.sort(proxies)
	local effective = source_visible or proxy_visible
	local signature = tostring(source_visible) .. ":" .. tostring(proxy_visible) .. ":" .. tostring(effective) .. ":" .. table.concat(proxies, ",")
	local track = player_render_tracks[player_index]
	if player_render_signatures[player_index] == signature then return end
	player_render_signatures[player_index] = signature
	track.samples[#track.samples + 1] = { current_beat, current_seconds, source_visible, proxy_visible, effective, proxies }
end

local loaded_roots = {}
local function init(actor)
	for _, child in ipairs(actor.children) do init(child) end
	run_command(actor, "Init")
end

for _, entry in ipairs(harness.entries) do
	local result = execute_file(normalize(entry.path))
	if type(result) ~= "table" then result = Def.ActorFrame {} end
	result._HarnessLayer = entry.layer
	result._HarnessLayerIndex = entry.index
	result._HarnessStartBeat = entry.start_beat
	local definition = register_definition(result)
	local parent_name = entry.layer == "foreground" and "SongForeground" or "SongBackground"
	local parent = actor_child(top_screen, parent_name)
	local actor = instantiate(result, parent)
	-- Foreground::LoadFromSong initializes each MakeActor result before loading
	-- the next file and before AddChild. Later scripts may consume the same RNG
	-- or read globals set by this root's InitCommand.
	init(actor)
	parent.children[#parent.children + 1] = actor
	local child_name = actor.__songlua_name
	parent.children_by_name[child_name] = parent.children_by_name[child_name] or {}
	table.insert(parent.children_by_name[child_name], actor)
	loaded_roots[#loaded_roots + 1] = { actor = actor, definition = definition, entry = entry }
	roots[#roots + 1] = { actor = actor, definition_id = definition.id, layer = entry.layer, layer_index = entry.index, start_beat = entry.start_beat }
end

for _, root in ipairs(loaded_roots) do visit(root.actor, function(actor) run_command(actor, "Begin") end) end
for _, root in ipairs(loaded_roots) do visit(root.actor, function(actor) run_command(actor, "On") end) end

local function crosses_action(previous, beat, first_frame)
	for _, item in ipairs(scheduled_beats) do
		local action_beat = tonumber(item.beat) or 0
		if action_beat <= beat + 0.000001 and (first_frame or action_beat > previous + 0.000001) then return true end
	end
	-- TaroNuke-style readers poll their sorted action table from a queued
	-- UpdateCommand.  The command can wake a frame after the authored beat, so
	-- keep capture enabled while its cursor still points at an overdue action.
	local action_index = tonumber(curaction)
	local pending = action_index and (mod_actions or {})[action_index] or nil
	local pending_beat = type(pending) == "table" and tonumber(pending[1]) or nil
	if pending_beat and pending_beat <= beat + 0.000001 then return true end
	for _, item in ipairs(mod_actions or {}) do
		local action_beat = type(item) == "table" and tonumber(item[1]) or nil
		if action_beat and action_beat <= beat + 0.000001
			and (first_frame or action_beat > previous + 0.000001) then return true end
	end
	return false
end

local beat_step = harness.beat_step
local previous_beat, previous_seconds, sample_beat, frame = 0, 0, 0, 0
local end_seconds = seconds_for_beat(harness.max_beat)
if harness.native_song_end and harness.max_beat >= harness.native_song_end.beat then
	end_seconds = math.max(end_seconds, harness.native_song_end.seconds)
end
while true do
	current_seconds = math.min(frame / UPDATE_FPS, end_seconds)
	if _ITG_SONG_POSITION then
		current_beat, current_bps, current_freeze, current_delay = _ITG_SONG_POSITION(current_seconds)
	else
		current_beat = beat_for_seconds(current_seconds)
	end
	_ITG_BEGIN_FRAME(current_beat)
	local sample_frame = current_beat + 0.000001 >= sample_beat
	capture_operations = sample_frame or frame <= 1 or current_freeze or current_delay
		or crosses_action(previous_beat, current_beat, frame == 0)
	local delta = _ITG_FLOAT(_ITG_FLOAT(current_seconds) - _ITG_FLOAT(previous_seconds))
	advance_actor(top_screen, delta)
	run_scheduled_beats()
	manual.run(loaded_roots)
	update_frames[#update_frames + 1] = { current_beat, current_seconds }
	-- Setter events are downsampled. Preserve own render state on every frame,
	-- recording only changes so quiet actors do not inflate the trace.
	for _, record in ipairs(runtime_actors) do
		local actor = actor_by_id[record.id]
		local alpha, visible = state_value(actor.state, "diffuse")[4], actor.state.visible ~= false
		local samples = record.render_state_samples or {}
		local previous = samples[#samples]
		local same_alpha = previous and (previous[2] == alpha or (previous[2] ~= previous[2] and alpha ~= alpha))
		if not previous or not same_alpha or previous[3] ~= visible then
			samples[#samples + 1] = { frame, alpha, visible }
		end
		record.render_state_samples = samples
	end
	for player_index, player in ipairs(tracked_players) do
		local values = {}
		for _, key in ipairs({ "x", "y", "z", "rotationx", "rotationz", "rotationy", "zoomx", "zoomy", "zoomz", "skewx", "skewy" }) do
			values[#values + 1] = state_value(player.state, key)
		end
		local signature = table.concat(values, ":")
		if player_transform_signatures[player_index] ~= signature then
			local track = player_render_tracks[player_index]
			track.transform_samples = track.transform_samples or {}
			table.insert(values, 1, frame)
			track.transform_samples[#track.transform_samples + 1] = values
			player_transform_signatures[player_index] = signature
		end
	end
	if sample_frame then
		for player_index, player in ipairs(tracked_players) do record_player_render(player_index, player) end
		record_projected_vertices(loaded_roots)
		while sample_beat <= current_beat + 0.000001 do sample_beat = sample_beat + beat_step end
	end
	-- Model animation and texture clocks advance at every Actor::Update.
	-- Observe each update independently of sparse beat sampling for other actors.
	world_matrix_cache, projection_cache, color_cache = {}, {}, {}
	for _, root in ipairs(loaded_roots) do visit(root.actor, manual.models.record) end
	previous_beat, previous_seconds = current_beat, current_seconds
	if emitted_events >= harness.max_events then break end
	if current_seconds >= end_seconds - 0.0000001 then break end
	frame = frame + 1
end
capture_operations = true

-- Update operations are sampled, so the last recorded setter can precede
-- the actor's final state. Snapshot own alpha and visibility after the replay.
for _, record in ipairs(runtime_actors) do
	local actor = actor_by_id[record.id]
	if actor then
		record.message_order = _ITG_MESSAGE_RANK(rawget(actor, "native_subscriber"))
		-- Keep raw floats while comparing successive frames, then encode the
		-- native nonfinite kind before JSON would replace it with null.
		for _, sample in ipairs(record.render_state_samples or {}) do
			sample[2] = safe_value(sample[2], 0, {})
		end
		record.final_render_state = {
			alpha = safe_value(state_value(actor.state, "diffuse")[4], 0, {}),
			visible = actor.state.visible ~= false,
		}
	end
end

for _, record in ipairs(external_actors) do
	local subscriber = rawget(actor_by_id[record.id], "native_subscriber")
	if subscriber then record.message_order = _ITG_MESSAGE_RANK(subscriber) end
end

local root_ids = {}
local root_layers = {}
for _, root in ipairs(roots) do
	root_ids[#root_ids + 1] = root.definition_id
	root_layers[#root_layers + 1] = {
		definition_id = root.definition_id,
		layer = root.layer,
		layer_index = root.layer_index,
		start_beat = root.start_beat,
	}
end

-- Serialization is finite and bounded by the event limit; do not charge its
-- table traversal against the song execution budget.
debug.sethook()
collectgarbage("collect")
local lua_sources = {}
for path in pairs(loaded_lua_files) do lua_sources[#lua_sources + 1] = path end
table.sort(lua_sources)
return json_encode({
	schema_version = 1,
	oracle = "itgmania_song_lua_headless_semantic_trace",
	arrow_timing = _ITG_TIMING_Y_OFFSET and "native" or "linear",
	song_clock = _ITG_SONG_POSITION and "native-song-timing" or "continuous-bpm",
	song_position = _ITG_NATIVE_SONG_POSITION and "native-music-seconds" or "synthetic-music-seconds",
	music_effect_clock = _ITG_NATIVE_SONG_POSITION and "native-music-seconds" or "synthetic-music-seconds",
	message_dispatch = "native-subscriber-pointer-order",
	wrapper_effects = "native-draw-stack",
	random_seed = random_seed,
	random_generator = "ITGmania MersenneTwister",
    random_reseeds = _ITG_RANDOM_RESEEDS,
	harness_version = harness.harness_version,
	itgmania_version = harness.itgmania_version,
	simfile = source_path(harness.simfile),
	loaded_lua_files = lua_sources,
    texture_requests = file_observations.textures,
    file_reads = file_observations.reads,
    directory_queries = file_observations.directories,
    file_writes = file_observations.writes,
	title = harness.title,
	theme = "headless",
	game = "dance",
	style = style_name,
	enabled_players = { true, not is_double },
	steps_type = harness.steps_type,
	difficulty = harness.difficulty,
	description = harness.description,
	display = { width = harness.display_width, height = harness.display_height, logical_width = harness.screen_width, logical_height = harness.screen_height },
	trace_until_beat = harness.max_beat,
	update_fps = UPDATE_FPS,
	bpm_segments = bpm_segments,
	end_position = { beat = current_beat, seconds = current_seconds,
		music_seconds = GAMESTATE:GetSongPosition():GetMusicSeconds() },
    native_song_end = harness.native_song_end,
    trace_until_seconds = end_seconds,
    calendar = { year=2026, month=10, day=1, hour=12, minute=0, second=0 },
	update_frames = update_frames,
	message_dispatches = manual.message_dispatches,
	capabilities = {
		actor_definitions = true, child_layer_order = true, command_execution = true,
		actor_method_calls = true, callback_calls = true, message_broadcasts = true,
		modifier_mutations = true, embedded_itgmania_lua = true, launches_itgmania = false,
		runtime_complete = #runtime_errors == 0,
        native_file_reads = true, native_directory_listing = true,
        isolated_file_writes = _ITG_SONG_WRITABLE,
		callback_operation_tracks = true, recurrent_command_sampling = true,
		external_actor_paths = true, player_render_samples = true,
		projected_vertex_samples = true, projected_draw_color_samples = true,
		manual_draw_frames = true, native_multi_vertex_primitives = true,
		native_model_primitives = true, native_model_geometry_buffers = true,
        actor_base_rotation = true,
        model_texture_matrix_scale = true,
        model_hardware_mesh_path = true,
        model_update_order = true,
        model_texture_bindings = true,
        model_texture_metadata = true,
        native_column_splines = true,
		sprite_texture_alias_samples = true,
		sprite_crop_samples = true,
		sprite_shadow_samples = true,
	},
	root_layers = root_layers,
	roots = root_ids,
	actor_definitions = definitions,
	runtime_actors = runtime_actors,
	external_actors = external_actors,
	player_render_tracks = player_render_tracks,
	projected_vertex_tracks = projected_vertex_tracks,
	model_geometry_tracks = manual.models.tracks,
	model_geometry_encoding = "column-buffer-v1",
	model_geometry_sample_clock = "update_frames",
	model_texture_units = 1,
	model_geometry_buffers = manual.models.buffers,
	model_geometry_buffer_stats = {buffers=#manual.models.buffers, indexed_buffers=manual.models.lookup_count,
		lookup_key_bytes=manual.models.lookup_bytes, hits=manual.models.buffer_hits,
		saturated_misses=manual.models.saturated_misses},
	manual_draw_frames = manual.frames,
	events = events,
	callback_operation_tracks = callback_operation_tracks,
	emitted_event_count = emitted_events,
	runtime_errors = runtime_errors,
	dropped_events = dropped_events,
})
