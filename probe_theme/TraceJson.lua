-- Keep the native JsonEncode representation, but never pass the complete trace
-- or a complete top-level tape to its Lua -> JsonCpp -> string conversion.
-- RageUtil.cpp treats a nonempty raw-length table as an array, and an empty
-- table as []; record encoding below remains entirely native.
local function write_song_lua_trace_result(document, result_path, spooled_events)
    local file = RageFileUtil:CreateRageFile()
    assert(file:Open(result_path, 2), "cannot open trace result")
    document.events = nil

    local fragments, fragment_bytes = {}, 0
    local buffer_bytes = 65536
    local function flush_metadata()
        if fragment_bytes == 0 then return end
        local chunk = table.concat(fragments)
        assert(file:Write(chunk) == #chunk, "cannot write trace metadata")
        fragments, fragment_bytes = {}, 0
    end
    local function metadata(fragment)
        if fragment_bytes + #fragment > buffer_bytes then flush_metadata() end
        if #fragment >= buffer_bytes then
            assert(file:Write(fragment) == #fragment, "cannot write trace metadata")
        else
            fragments[#fragments + 1] = fragment
            fragment_bytes = fragment_bytes + #fragment
        end
    end
    local function encoded(value)
        return JsonEncode(value, true):gsub("%s+$", "")
    end

    -- These are the recorder's named metadata fields. Match native JsonCpp's
    -- sorted object order and leave the existing event spool as the last field.
    local keys = {}
    for key in pairs(document) do keys[#keys + 1] = key end
    table.sort(keys)
    metadata("{")
    for member, key in ipairs(keys) do
        if member > 1 then metadata(",") end
        metadata(encoded(key))
        metadata(":")
        local value = document[key]
        if type(value) == "table" and #value > 0 then
            metadata("[")
            for index = 1, #value do
                if index > 1 then metadata(",") end
                metadata(encoded(rawget(value, index)))
            end
            metadata("]")
        else
            metadata(encoded(value))
        end
    end
    if #keys > 0 then metadata(",") end
    metadata('"events":[')
    flush_metadata()
    if spooled_events > 0 then
        local spool = RageFileUtil:CreateRageFile()
        assert(spool:Open(result_path .. ".events", 1), "cannot read event spool")
        while true do
            local chunk = spool:ReadBytes(1048576)
            if #chunk == 0 then break end
            assert(file:Write(chunk) == #chunk, "cannot write trace events")
        end
        spool:Close(); spool:destroy()
    end
    assert(file:Write("]}") == 2, "cannot finish trace result")
    -- As before, RageFile atomically publishes the result on Close.
    file:Close(); file:destroy()
end
