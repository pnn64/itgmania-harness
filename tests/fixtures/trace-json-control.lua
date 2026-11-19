-- Appended to the exact production TraceJson.lua helper by oracle_cli.rs.
-- JsonEncode is the pinned native RageUtil implementation in this process.
local native_encode = JsonEncode
local native_files = RageFileUtil
local function encode(value)
    return native_encode(value, true):gsub("%s+$", "")
end

local records = {}
for index = 1, 513 do
    records[index] = {
        sequence = index,
        vertices = {{1, 2, 3, 0.03333333507180214}, {-0.5, 0, 16777218}},
        state = {filter = true, wrap = false, empty = {}},
        text = index == 257 and string.rep("large record ", 16384) or "quote\" slash\\ newline\n nul" .. string.char(0) .. " λ 雪",
    }
end
local document = {
    schema_version = 17, oracle = "native-json-equivalence",
    empty = {}, flags = {false, true, false}, holes = {[1] = 1, [3] = 3},
    fractional = {0.03333333507180214, -0.00000011920928955078125, 4294967296, -2147483649},
    nested = {empty = {}, sentinel = false, number = {type = "number", value = "nan"}},
    drawable_samples = records, events = {"replaced by the spool"},
}
local event_fragments = {}
for index = 1, 3000 do
    event_fragments[index] = encode({sequence = index, text = string.rep("e", 1024)})
end
local event_spool = table.concat(event_fragments, ",")
document.events = nil
local old_metadata = encode(document)
local expected = old_metadata:sub(1, -2) .. ',"events":[' .. event_spool .. "]}"

local function exercise(value, spool_bytes, count, failure)
    local capture = {writes = {}, metadata_writes = 0, largest_metadata_write = 0,
        spool_reads = 0, encoded_values = 0, published = false, phase = "metadata"}
    local whole_tapes = {}
    for _, tape in pairs(value) do
        if type(tape) == "table" and #tape > 0 then whole_tapes[tape] = true end
    end
    JsonEncode = function(part, minified)
        assert(part ~= value and not whole_tapes[part], "encoded a complete tape")
        assert(minified == true)
        capture.encoded_values = capture.encoded_values + 1
        return native_encode(part, minified)
    end
    RageFileUtil = {CreateRageFile = function()
        return {
            Open = function(self, path, mode)
                self.mode = mode
                if mode == 2 then
                    assert(path == "trace-result.json")
                    return failure ~= "result-open"
                end
                assert(path == "trace-result.json.events" and mode == 1)
                self.position = 1
                return failure ~= "spool-open"
            end,
            Write = function(self, bytes)
                assert(self.mode == 2)
                if failure == capture.phase then return #bytes - 1 end
                if capture.phase == "metadata" then
                    capture.metadata_writes = capture.metadata_writes + 1
                    capture.largest_metadata_write = math.max(capture.largest_metadata_write, #bytes)
                end
                capture.writes[#capture.writes + 1] = bytes
                return #bytes
            end,
            ReadBytes = function(self, maximum)
                assert(self.mode == 1 and maximum == 1048576)
                local bytes = spool_bytes:sub(self.position, self.position + maximum - 1)
                self.position = self.position + #bytes
                if #bytes > 0 then
                    capture.phase = "events"
                    capture.spool_reads = capture.spool_reads + 1
                else
                    capture.phase = "finish"
                end
                return bytes
            end,
            Close = function(self)
                if self.mode == 2 then capture.published = true end
            end,
            destroy = function() end,
        }
    end}
    local ok, message = pcall(write_song_lua_trace_result, value, "trace-result.json", count)
    JsonEncode, RageFileUtil = native_encode, native_files
    capture.ok, capture.error = ok, ok and nil or tostring(message)
    capture.output = table.concat(capture.writes)
    return capture
end

local complete = exercise(document, event_spool, 3000)
assert(complete.ok, complete.error)
assert(complete.published and complete.output == expected, "native JSON bytes changed")
assert(complete.metadata_writes >= 3 and complete.spool_reads >= 3)
assert(complete.encoded_values > #records)
assert(complete.largest_metadata_write > 65536)
assert(complete.largest_metadata_write <= #encode(records[257]))

local empty = {empty = {}, nested = {empty = {}}, scalar = false}
local empty_expected = encode(empty):sub(1, -2) .. ',"events":[]}'
local empty_result = exercise(empty, "", 0)
assert(empty_result.ok and empty_result.output == empty_expected)
assert(empty_result.spool_reads == 0)

local failures = {}
for _, entry in ipairs({
    {"result-open", "cannot open trace result"},
    {"metadata", "cannot write trace metadata"},
    {"spool-open", "cannot read event spool"},
    {"events", "cannot write trace events"},
    {"finish", "cannot finish trace result"},
}) do
    local failed = exercise(document, event_spool, 3000, entry[1])
    assert(not failed.ok and not failed.published, entry[1])
    assert(failed.error:find(entry[2], 1, true), failed.error)
    failures[entry[1]] = failed.error
end
local unsupported = exercise({unsupported = function() end}, "", 0)
assert(not unsupported.ok and not unsupported.published)
assert(unsupported.error:find("not JSON serializable", 1, true), unsupported.error)
failures.encoder = unsupported.error

local directory = GAMESTATE:GetCurrentSong():GetSongDir()
local function retain(name, bytes)
    local file = native_files:CreateRageFile()
    assert(file:Open(directory .. name, 2), name)
    assert(file:Write(bytes) == #bytes, name)
    file:Close(); file:destroy()
end
retain("expected.json", expected)
retain("streamed.json", complete.output)
retain("control-receipt.json", encode({
    passed = true, byte_equivalent = true, bytes = #expected,
    metadata_writes = complete.metadata_writes,
    largest_metadata_write = complete.largest_metadata_write,
    encoded_values = complete.encoded_values, spool_reads = complete.spool_reads,
    failure_cases = failures, empty_spool_checked = true,
}))
return Def.Actor{Name="NativeJsonStreamingControl"}
