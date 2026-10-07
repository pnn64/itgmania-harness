-- Host bindings only. All chart calculations and BPM display formatting run
-- in the selected, unmodified Simply Love scripts at music rate 1.
PLAYER_1, PLAYER_2 = 'PlayerNumber_P1', 'PlayerNumber_P2'
SL = {P1={Streams={}}, P2={Streams={}}, Global={GameMode='ITG', ColumnCueMinTime=0, ActiveModifiers={MusicRate=1}}}
GAMESTATE = {IsCourseMode=function() return false end}
CRYPTMAN = {SHA1String=_SHA1}
-- Same contract as the fallback theme's Scripts/02 Enum.lua.
function ToEnumShortString(value)
    local pos = string.find(value, '_')
    assert(pos, "'" .. value .. "' is not an enum value")
    return string.sub(value, pos+1)
end
function ivalues(t)
    local i = 0
    return function() i=i+1; return t[i] end
end
RageFileUtil = {}
function RageFileUtil.CreateRageFile()
    return {
        Open=function(self, filename, mode)
            assert(mode == 1, 'theme requested writable file')
            self.contents = _READ_FILE(filename)
            return true
        end,
        Read=function(self) return self.contents end,
        destroy=function(self) self.contents=nil end
    }
end
-- Inspect the selected theme's own parser functions to capture their hash input.
-- The chart selection, minimization, and BPM normalization stay in that script.
local function HashBPMs(steps)
    local fn = ComputeChartHash or ParseChartInfo
    local read, select
    for i=1,math.huge do
        local name, value = debug.getupvalue(fn, i)
        if not name then break end
        if name == 'GetSimfileString' then read = value end
        if name == 'GetSimfileChartString' then select = value end
    end
    assert(read and select, 'theme does not expose its chart source parser')
    local bytes, ext = read(steps)
    local notes, bpms = select(bytes, ToEnumShortString(steps:GetStepsType()):gsub('_','-'):lower(),
        ToEnumShortString(steps:GetDifficulty()), steps:GetDescription(), ext)
    assert(type(notes) == 'string' and type(bpms) == 'string', 'theme could not select the chart source')
    return bpms
end
function _CAPTURE_THEME()
    local result = {status='complete', errors={}, music_rate=1, players={}}
    local function capture(label, fn)
        local ok, value = pcall(fn)
        if not ok then
            result.status = 'partial'
            result.errors[#result.errors+1] = label .. ': ' .. tostring(value)
        end
        return ok, value
    end
    for _, pn in ipairs({'P1','P2'}) do
        -- Do not let the opposite-player identity cache certify a wrong chart.
        SL.P1.Streams, SL.P2.Streams = {}, {}
        player = pn == 'P1' and PLAYER_1 or PLAYER_2
        local p = {player=pn}
        capture(pn .. '.ParseChartInfo', function() ParseChartInfo(_STEPS, pn) end)
        capture(pn .. '.ComputeChartHash', function()
            if type(ComputeChartHash) == 'function' then ComputeChartHash(_STEPS, pn) end
            assert(type(SL[pn].Streams.Hash) == 'string' and SL[pn].Streams.Hash:match('^%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x%x$'), 'theme returned an empty or invalid hash')
            assert(not _AMBIGUOUS, 'theme chart identity matches multiple native charts')
        end)
        capture(pn .. '.hash_bpms', function() p.hash_bpms = HashBPMs(_STEPS) end)
        capture(pn .. '.breakdowns', function()
            p.stream_sequences = GetStreamSequences(SL[pn].Streams.NotesPerMeasure, 16)
            p.breakdowns = {}
            for level=0,3 do p.breakdowns[#p.breakdowns+1] = GenerateBreakdownText(pn, level) end
            p.total_stream_measures, p.total_break_measures = GetTotalStreamAndBreakMeasures(pn)
        end)
        p.streams = SL[pn].Streams
        -- Temporary oracle filenames are not chart results.
        p.streams.Filename = nil
        result.players[#result.players+1] = p
    end
    capture('StringifyDisplayBPMs', function()
        result.display_bpms = GetDisplayBPMs(PLAYER_1, _STEPS, 1)
        result.display_bpm_text = StringifyDisplayBPMs(PLAYER_1, _STEPS, 1)
        assert(type(result.display_bpm_text) == 'string' and result.display_bpm_text ~= '', 'theme returned empty display BPM text')
    end)
    return result
end
