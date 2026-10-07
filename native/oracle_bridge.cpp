#include "oracle_bridge.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstring>
#include <exception>
#include <filesystem>
#include <limits>
#include <map>
#include <stdexcept>
#include <string>
#include <vector>

#include "Font.h"
#include "BackgroundUtil.h"
#include "Game.h"
#include "GameConstantsAndTypes.h"
#include "NoteData.h"
#include "NoteTypes.h"
#include "NotesLoaderSM.h"
#include "NotesLoaderSSC.h"
#include "NoteSkinManager.h"
#include "PlayerOptions.h"
#include "RageMath.h"
#include "Song.h"
#include "Steps.h"
#include "TimingData.h"
#include "TimingSegments.h"
#include "RageTexture.h"
#include "RageTypes.h"
#include "RageUtil.h"
#include "ThemeManager.h"
#include "runtime_stubs.h"

void install_option_queries(lua_State* state);
void install_actor_math(lua_State* state);
std::string harness_chart_theme(Steps& steps, const std::string& theme,
    const std::string& host, bool ambiguous);

namespace {

constexpr uint8_t kMagic[] = {'I', 'T', 'G', 'O', 'R', 'C', 'L', 0};
constexpr uint32_t kWireVersion = 5;
constexpr uint8_t kFontMagic[] = {'I', 'T', 'G', 'F', 'O', 'N', 'T', 0};
constexpr uint32_t kFontWireVersion = 1;
constexpr uint8_t kNoteSkinMagic[] = {'I', 'T', 'G', 'N', 'S', 'K', 'N', 0};
constexpr uint32_t kNoteSkinWireVersion = 1;
constexpr uint8_t kSongLuaMagic[] = {'I', 'T', 'G', 'S', 'L', 'U', 'A', 0};
constexpr uint32_t kSongLuaWireVersion = 2;
constexpr uint8_t kSongLuaSemanticMagic[] = {'I', 'T', 'G', 'S', 'E', 'M', 0, 0};
constexpr uint32_t kSongLuaSemanticWireVersion = 2;

uint32_t wire_len(size_t len, const char* field) {
  if (len > std::numeric_limits<uint32_t>::max()) {
    throw std::length_error(std::string(field) + " exceeds wire limit");
  }
  return static_cast<uint32_t>(len);
}

class Writer {
 public:
  void byte(uint8_t value) { bytes_.push_back(value); }

  void u32(uint32_t value) {
    byte(static_cast<uint8_t>(value));
    byte(static_cast<uint8_t>(value >> 8));
    byte(static_cast<uint8_t>(value >> 16));
    byte(static_cast<uint8_t>(value >> 24));
  }

  void i32(int32_t value) { u32(static_cast<uint32_t>(value)); }

  void f32(float value) {
    uint32_t bits;
    static_assert(sizeof(bits) == sizeof(value));
    std::memcpy(&bits, &value, sizeof(bits));
    u32(bits);
  }

  void string(const std::string& value) {
    u32(wire_len(value.size(), "oracle string"));
    bytes_.insert(bytes_.end(), value.begin(), value.end());
  }

  void header(uint8_t status) {
    bytes_.insert(bytes_.end(), std::begin(kMagic), std::end(kMagic));
    u32(kWireVersion);
    byte(status);
  }

  void font_header(uint8_t status) {
    bytes_.insert(bytes_.end(), std::begin(kFontMagic), std::end(kFontMagic));
    u32(kFontWireVersion);
    byte(status);
  }

  void noteskin_header(uint8_t status) {
    bytes_.insert(bytes_.end(), std::begin(kNoteSkinMagic),
                  std::end(kNoteSkinMagic));
    u32(kNoteSkinWireVersion);
    byte(status);
  }

  void song_lua_header(uint8_t status) {
    bytes_.insert(bytes_.end(), std::begin(kSongLuaMagic),
                  std::end(kSongLuaMagic));
    u32(kSongLuaWireVersion);
    byte(status);
  }

  void song_lua_semantic_header(uint8_t status) {
    bytes_.insert(bytes_.end(), std::begin(kSongLuaSemanticMagic),
                  std::end(kSongLuaSemanticMagic));
    u32(kSongLuaSemanticWireVersion);
    byte(status);
  }

  ItgOracleBuffer release() {
    auto* data = new uint8_t[bytes_.size()];
    std::memcpy(data, bytes_.data(), bytes_.size());
    return {data, bytes_.size()};
  }

 private:
  std::vector<uint8_t> bytes_;
};

class Reader {
 public:
  Reader(const uint8_t* data, size_t len) : data_(data), len_(len) {}

  uint32_t u32() {
    require(4);
    const uint32_t value = static_cast<uint32_t>(data_[offset_]) |
                           (static_cast<uint32_t>(data_[offset_ + 1]) << 8) |
                           (static_cast<uint32_t>(data_[offset_ + 2]) << 16) |
                           (static_cast<uint32_t>(data_[offset_ + 3]) << 24);
    offset_ += 4;
    return value;
  }

  float f32() {
    const uint32_t bits = u32();
    float value;
    static_assert(sizeof(bits) == sizeof(value));
    std::memcpy(&value, &bits, sizeof(value));
    return value;
  }

  std::string string() {
    const uint32_t size = u32();
    require(size);
    const char* start = reinterpret_cast<const char*>(data_ + offset_);
    offset_ += size;
    return std::string(start, size);
  }

  bool empty() const { return offset_ == len_; }

 private:
  void require(size_t size) const {
    if (size > len_ - offset_) {
      throw std::runtime_error("truncated noteskin oracle request");
    }
  }

  const uint8_t* data_;
  size_t len_;
  size_t offset_ = 0;
};

struct SongLuaSemanticEntry {
  std::string path;
  std::string layer;
  uint32_t index;
  float start_beat;
};

struct SongLuaSemanticBpm {
  float beat;
  float bpm;
};

struct SongLuaSemanticRequest {
  std::string simfile;
  std::string song_dir;
  std::string title;
  std::string difficulty;
  std::string steps_type;
  std::string description;
  std::string harness_version;
  std::string itgmania_version;
  float max_beat;
  float bpm;
  float beat_step;
  std::vector<SongLuaSemanticBpm> bpm_segments;
  float screen_width;
  float screen_height;
  float display_width;
  float display_height;
  uint32_t max_events;
  uint32_t max_commands;
  uint32_t max_queued_per_tick;
  std::vector<SongLuaSemanticEntry> entries;
  std::vector<std::string> texture_files;
};

SongLuaSemanticRequest read_song_lua_semantic_request(
    const uint8_t* data, size_t len) {
  Reader input(data, len);
  SongLuaSemanticRequest request;
  request.simfile = input.string();
  request.song_dir = input.string();
  request.title = input.string();
  request.difficulty = input.string();
  request.steps_type = input.string();
  request.description = input.string();
  request.harness_version = input.string();
  request.itgmania_version = input.string();
  request.max_beat = input.f32();
  request.bpm = input.f32();
  request.beat_step = input.f32();
  const uint32_t bpm_count = input.u32();
  request.bpm_segments.reserve(bpm_count);
  for (uint32_t index = 0; index < bpm_count; ++index) {
    request.bpm_segments.push_back({input.f32(), input.f32()});
  }
  request.screen_width = input.f32();
  request.screen_height = input.f32();
  request.display_width = input.f32();
  request.display_height = input.f32();
  request.max_events = input.u32();
  request.max_commands = input.u32();
  request.max_queued_per_tick = input.u32();
  const uint32_t entry_count = input.u32();
  request.entries.reserve(entry_count);
  for (uint32_t index = 0; index < entry_count; ++index) {
    request.entries.push_back(
        {input.string(), input.string(), input.u32(), input.f32()});
  }
  const uint32_t texture_count = input.u32();
  request.texture_files.reserve(texture_count);
  for (uint32_t index = 0; index < texture_count; ++index) {
    request.texture_files.push_back(input.string());
  }
  if (!input.empty()) {
    throw std::runtime_error("song Lua semantic request has trailing bytes");
  }
  return request;
}

void lua_field(lua_State* state, const char* key, const std::string& value) {
  lua_pushlstring(state, value.data(), value.size());
  lua_setfield(state, -2, key);
}

void lua_field(lua_State* state, const char* key, float value) {
  lua_pushnumber(state, value);
  lua_setfield(state, -2, key);
}

void lua_field(lua_State* state, const char* key, uint32_t value) {
  lua_pushinteger(state, static_cast<lua_Integer>(value));
  lua_setfield(state, -2, key);
}

void push_song_lua_semantic_request(lua_State* state,
                                    const SongLuaSemanticRequest& request) {
  lua_newtable(state);
  lua_field(state, "simfile", request.simfile);
  lua_field(state, "song_dir", request.song_dir);
  lua_field(state, "title", request.title);
  lua_field(state, "difficulty", request.difficulty);
  lua_field(state, "steps_type", request.steps_type);
  lua_field(state, "description", request.description);
  lua_field(state, "harness_version", request.harness_version);
  lua_field(state, "itgmania_version", request.itgmania_version);
  lua_field(state, "max_beat", request.max_beat);
  lua_field(state, "bpm", request.bpm);
  lua_field(state, "beat_step", request.beat_step);
  // ArrowEffects' Lua GetRotationX takes (ps, offset, column), but legacy
  // multitap calls (ps, offset, 0, lane). Check the native layout and read the
  // aliased member safely instead of evaluating m_fConfusionX[-1] (UB).
  const PlayerOptions options;
  if (options.m_SpeedfMovesZ + 16 != options.m_fConfusionX) {
    throw std::runtime_error("legacy GetRotationX PlayerOptions layout changed");
  }
  lua_field(state, "legacy_rotation_x", options.m_SpeedfMovesZ[15] * 180.0f / PI);
  lua_createtable(state, static_cast<int>(request.bpm_segments.size()), 0);
  for (size_t index = 0; index < request.bpm_segments.size(); ++index) {
    const SongLuaSemanticBpm& segment = request.bpm_segments[index];
    lua_createtable(state, 2, 0);
    lua_pushnumber(state, segment.beat);
    lua_rawseti(state, -2, 1);
    lua_pushnumber(state, segment.bpm);
    lua_rawseti(state, -2, 2);
    lua_rawseti(state, -2, static_cast<int>(index + 1));
  }
  lua_setfield(state, -2, "bpm_segments");
  lua_field(state, "screen_width", request.screen_width);
  lua_field(state, "screen_height", request.screen_height);
  lua_field(state, "display_width", request.display_width);
  lua_field(state, "display_height", request.display_height);
  lua_field(state, "max_events", request.max_events);
  lua_field(state, "max_commands", request.max_commands);
  lua_field(state, "max_queued_per_tick", request.max_queued_per_tick);
  lua_createtable(state, static_cast<int>(request.entries.size()), 0);
  for (size_t index = 0; index < request.entries.size(); ++index) {
    const SongLuaSemanticEntry& entry = request.entries[index];
    lua_newtable(state);
    lua_field(state, "path", entry.path);
    lua_field(state, "layer", entry.layer);
    lua_field(state, "index", entry.index);
    lua_field(state, "start_beat", entry.start_beat);
    lua_rawseti(state, -2, static_cast<int>(index + 1));
  }
  lua_setfield(state, -2, "entries");
  lua_createtable(state, static_cast<int>(request.texture_files.size()), 0);
  for (size_t index = 0; index < request.texture_files.size(); ++index) {
    lua_pushlstring(state, request.texture_files[index].data(),
                    request.texture_files[index].size());
    lua_rawseti(state, -2, static_cast<int>(index + 1));
  }
  lua_setfield(state, -2, "texture_files");
  lua_setglobal(state, "_HARNESS");
}

thread_local int64_t song_lua_instruction_budget = 0;
thread_local double song_lua_frame_beat = 0;
thread_local bool song_lua_first_frame = true;
constexpr int64_t SONG_LUA_INSTRUCTION_LIMIT = 200000000;

void song_lua_instruction_hook(lua_State* state, lua_Debug*) {
  song_lua_instruction_budget -= 10000;
  if (song_lua_instruction_budget <= 0) {
    luaL_error(state, "song Lua instruction budget exhausted at beat %f",
               song_lua_frame_beat);
  }
}

std::string lua_error_text(lua_State* state) {
  const char* message = lua_tostring(state, -1);
  return message == nullptr ? "unknown Lua error" : message;
}

ItgOracleBuffer song_lua_semantic_error_buffer(const std::string& message) {
  Writer out;
  out.song_lua_semantic_header(1);
  out.string(message);
  return out.release();
}

struct MetricRequest {
  std::string section;
  std::string key;
};

struct PathRequest {
  std::string button;
  std::string element;
};

struct NoteSkinRequest {
  std::vector<MetricRequest> metrics;
  std::vector<PathRequest> paths;
};

NoteSkinRequest read_noteskin_request(const uint8_t* data, size_t len) {
  Reader input(data, len);
  NoteSkinRequest request;
  const uint32_t metric_count = input.u32();
  request.metrics.reserve(metric_count);
  for (uint32_t index = 0; index < metric_count; ++index) {
    request.metrics.push_back({input.string(), input.string()});
  }
  const uint32_t path_count = input.u32();
  request.paths.reserve(path_count);
  for (uint32_t index = 0; index < path_count; ++index) {
    request.paths.push_back({input.string(), input.string()});
  }
  if (!input.empty()) {
    throw std::runtime_error("noteskin oracle request has trailing bytes");
  }
  return request;
}

class CurrentPathGuard {
 public:
  explicit CurrentPathGuard(const std::filesystem::path& path)
      : original_(std::filesystem::current_path()) {
    std::filesystem::current_path(path);
  }
  ~CurrentPathGuard() {
    std::error_code ignored;
    std::filesystem::current_path(original_, ignored);
  }

 private:
  std::filesystem::path original_;
};

class NoteSkinGlobalGuard {
 public:
  explicit NoteSkinGlobalGuard(NoteSkinManager* manager) { NOTESKIN = manager; }
  ~NoteSkinGlobalGuard() { NOTESKIN = nullptr; }
};

void write_segment(Writer& out, const TimingSegment& segment) {
  out.i32(static_cast<int32_t>(segment.GetType()));
  out.i32(segment.GetRow());
  out.f32(segment.GetBeat());

  const std::vector<float> values = segment.GetValues();
  out.u32(wire_len(values.size(), "timing segment value count"));
  for (float value : values) {
    out.f32(value);
  }

  if (segment.GetType() == SEGMENT_LABEL) {
    out.byte(1);
    out.string(static_cast<const LabelSegment&>(segment).GetLabel());
  } else {
    out.byte(0);
  }
}

uint32_t note_count(const NoteData& notes) {
  uint32_t count = 0;
  FOREACH_NONEMPTY_ROW_ALL_TRACKS(notes, row) {
    for (int track = 0; track < notes.GetNumTracks(); ++track) {
      if (notes.GetTapNote(track, row).type != TapNoteType_Empty) {
        if (count == std::numeric_limits<uint32_t>::max()) {
          throw std::length_error("note count exceeds wire limit");
        }
        ++count;
      }
    }
  }
  return count;
}

void write_note_data(Writer& out, Steps& steps) {
  NoteData notes;
  steps.GetNoteData(notes);
  out.u32(static_cast<uint32_t>(notes.GetNumTracks()));
  out.u32(note_count(notes));
  FOREACH_NONEMPTY_ROW_ALL_TRACKS(notes, row) {
    for (int track = 0; track < notes.GetNumTracks(); ++track) {
      const TapNote& note = notes.GetTapNote(track, row);
      if (note.type == TapNoteType_Empty) {
        continue;
      }
      out.i32(row);
      out.f32(NoteRowToBeat(row));
      out.i32(track);
      out.i32(static_cast<int32_t>(note.type));
      out.i32(static_cast<int32_t>(note.subType));
      out.i32(static_cast<int32_t>(note.source));
      out.i32(static_cast<int32_t>(note.pn));
      out.i32(note.iDuration);
      out.f32(NoteRowToBeat(note.iDuration));
      out.f32(note.fAttackDurationSeconds);
      out.i32(note.iKeysoundIndex);
      out.string(note.sAttackModifiers);
    }
  }
}

void write_stats(Writer& out, Steps& steps) {
  steps.CalculateStepStats(0.0F);
  out.u32(NUM_PLAYERS);
  for (int player = 0; player < NUM_PLAYERS; ++player) {
    const PlayerNumber pn = static_cast<PlayerNumber>(player);
    out.i32(player);
    const RadarValues& radar = steps.GetRadarValues(pn);
    out.u32(NUM_RadarCategory);
    for (int category = 0; category < NUM_RadarCategory; ++category) {
      out.f32(radar[category]);
    }
    const TechCounts& tech = steps.GetTechCounts(pn);
    out.u32(NUM_TechCountsCategory);
    for (int category = 0; category < NUM_TechCountsCategory; ++category) {
      out.f32(tech[category]);
    }
    const auto& notes = steps.GetNotesPerMeasure(pn);
    out.u32(wire_len(notes.size(), "measure count"));
    for (int count : notes) out.i32(count);
    const auto& nps = steps.GetNpsPerMeasure(pn);
    out.u32(wire_len(nps.size(), "NPS count"));
    for (float value : nps) out.f32(value);
    out.f32(steps.GetPeakNps(pn));
  }
}

void write_chart(Writer& out, Steps& steps, const Song& song) {
  TimingData* const timing = steps.GetTimingData();
  timing->TidyUpData(false);

  out.string(song.m_sMainTitle);
  out.string(song.m_sSubTitle);
  out.string(song.m_sArtist);
  out.string(song.m_sMainTitleTranslit);
  out.string(song.m_sSubTitleTranslit);
  out.string(song.m_sArtistTranslit);
  out.string(steps.GetCredit());
  out.string(steps.GetDescription());
  out.i32(static_cast<int32_t>(steps.m_StepsType));
  out.string(steps.m_StepsTypeStr);
  out.i32(static_cast<int32_t>(steps.GetDifficulty()));
  out.i32(steps.GetMeter());

  float actual_min = 0.0F;
  float actual_max = 0.0F;
  timing->GetActualBPM(actual_min, actual_max);
  DisplayBpms display;
  steps.GetDisplayBpms(display);
  out.f32(actual_min);
  out.f32(actual_max);
  out.f32(display.GetMin());
  out.f32(display.GetMax());

  out.f32(timing->m_fBeat0OffsetInSeconds);
  out.f32(timing->m_fBeat0GroupOffsetInSeconds);

  uint32_t segment_count = 0;
  for (int type = 0; type < NUM_TimingSegmentType; ++type) {
    const uint32_t type_count = wire_len(
        timing->GetTimingSegments(static_cast<TimingSegmentType>(type)).size(),
        "timing segment count");
    if (type_count > std::numeric_limits<uint32_t>::max() - segment_count) {
      throw std::length_error("timing segment count exceeds wire limit");
    }
    segment_count += type_count;
  }
  out.u32(segment_count);
  for (int type = 0; type < NUM_TimingSegmentType; ++type) {
    for (const TimingSegment* segment :
         timing->GetTimingSegments(static_cast<TimingSegmentType>(type))) {
      write_segment(out, *segment);
    }
  }
  const bool notes_supported = steps.m_StepsType != StepsType_Invalid;
  if (notes_supported) {
    write_note_data(out, steps);
    write_stats(out, steps);
  } else {
    // The loader preserves unknown chart types. Do not ask NoteData to create
    // zero tracks, and do not substitute any statistics for these charts.
    out.u32(0);  // no available tracks
    out.u32(0);  // no available note entries
    out.u32(0);  // no available player statistics
  }
  out.string(steps.GetGrooveStatsHash());
  out.i32(steps.GetGrooveStatsHashVersion());
  std::vector<std::string> hash_bpms;
  for (const TimingSegment* segment : timing->GetTimingSegments(SEGMENT_BPM)) {
    hash_bpms.push_back(NormalizeDecimal(segment->GetBeat()) + "=" +
        NormalizeDecimal(ToBPM(segment)->GetBPM()));
  }
  out.string(join(",", hash_bpms));
  if (notes_supported) {
    NoteData notes;
    steps.GetNoteData(notes);
    out.f32(timing->GetElapsedTimeFromBeat(notes.GetFirstBeat()));
    out.f32(timing->GetElapsedTimeFromBeat(notes.GetLastBeat()));
  } else {
    out.f32(0);  // decoded as unavailable, not reference values
    out.f32(0);
  }
}

bool load_song(const std::string& path, Song& song) {
  song.m_sSongFileName = path;
  song.SetSongDir(std::filesystem::path(path).parent_path().string());
  std::string extension = std::filesystem::path(path).extension().string();
  for (char& ch : extension) {
    ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  }
  if (extension == ".ssc" || extension == ".ats") {
    SSCLoader loader;
    return loader.LoadFromSimfile(path, song, false);
  }
  if (extension == ".sm" || extension == ".sma") {
    SMLoader loader;
    return loader.LoadFromSimfile(path, song, false);
  }
  throw std::runtime_error("supported extensions are .sm, .sma, .ssc, and .ats");
}

std::filesystem::path find_fonts_root(const std::filesystem::path& font_path) {
  std::filesystem::path current = font_path.parent_path();
  while (!current.empty()) {
    std::string name = current.filename().string();
    for (char& ch : name) {
      ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
    }
    if (name == "fonts") return current;
    const std::filesystem::path parent = current.parent_path();
    if (parent == current) break;
    current = parent;
  }
  throw std::runtime_error("font path must be inside a theme Fonts directory");
}

uint32_t glyph_frame(const glyph& value) {
  const FontPage& page = *value.m_pPage;
  if (page.m_aGlyphs.empty() || &value < page.m_aGlyphs.data() ||
      &value >= page.m_aGlyphs.data() + page.m_aGlyphs.size()) {
    throw std::runtime_error("ITGmania returned a glyph outside its font page");
  }
  return wire_len(static_cast<size_t>(&value - page.m_aGlyphs.data()),
                  "glyph frame index");
}

struct ProbedGlyph {
  wchar_t codepoint;
  const glyph* value;
  bool available;
  uint32_t page;
  int32_t pen_x;
};

std::wstring mapped_font_text(const Font& font) {
  std::wstring text;
  wchar_t missing = 0;
  for (uint32_t codepoint = 1; codepoint <= 0xFFFF; ++codepoint) {
    if (codepoint >= 0xD800 && codepoint <= 0xDFFF) continue;
    const wchar_t character = static_cast<wchar_t>(codepoint);
    const std::wstring one(1, character);
    if (font.FontCompleteForString(one)) {
      text.push_back(character);
    } else if (missing == 0 && codepoint >= 0x0378 &&
               character != FONT_DEFAULT_GLYPH) {
      missing = character;
    }
  }
  text.push_back(FONT_DEFAULT_GLYPH);
  if (missing != 0) text.push_back(missing);
  return text;
}

void write_font_page(Writer& out, const FontPage& page) {
  const RageTexture& texture = *page.m_FontPageTextures.m_pTextureMain;
  out.string(texture.GetID().filename);
  out.string(texture.GetID().AdditionalTextureHints);
  const RageTexture* const stroke = page.m_FontPageTextures.m_pTextureStroke;
  out.byte(stroke == nullptr ? 0 : 1);
  if (stroke != nullptr) out.string(stroke->GetID().filename);
  out.i32(texture.GetSourceWidth());
  out.i32(texture.GetSourceHeight());
  out.i32(texture.GetImageWidth());
  out.i32(texture.GetImageHeight());
  out.i32(texture.GetTextureWidth());
  out.i32(texture.GetTextureHeight());
  out.i32(texture.GetFramesWide());
  out.i32(texture.GetFramesHigh());
  out.i32(page.m_iHeight);
  out.i32(page.m_iLineSpacing);
  out.f32(page.m_fVshift);
  out.i32(page.m_iDrawExtraPixelsLeft);
  out.i32(page.m_iDrawExtraPixelsRight);
}

void write_probed_glyph(Writer& out, const ProbedGlyph& probe) {
  const glyph& value = *probe.value;
  out.u32(static_cast<uint32_t>(probe.codepoint));
  out.byte(probe.available ? 1 : 0);
  out.u32(probe.page);
  out.u32(glyph_frame(value));
  out.i32(probe.pen_x);
  out.i32(value.m_iHadvance);
  out.f32(value.m_fWidth);
  out.f32(value.m_fHeight);
  out.f32(value.m_fHshift);
  out.f32(value.m_pPage->m_fVshift);
  out.f32(value.m_TexRect.left);
  out.f32(value.m_TexRect.top);
  out.f32(value.m_TexRect.right);
  out.f32(value.m_TexRect.bottom);
}

ItgOracleBuffer font_error_buffer(const std::string& message) {
  Writer out;
  out.font_header(1);
  out.string(message);
  return out.release();
}

ItgOracleBuffer noteskin_error_buffer(const std::string& message) {
  Writer out;
  out.noteskin_header(1);
  out.string(message);
  return out.release();
}

ItgOracleBuffer song_lua_error_buffer(const std::string& message) {
  Writer out;
  out.song_lua_header(1);
  out.string(message);
  return out.release();
}

void write_song_lua_change(Writer& out, uint32_t layer, uint32_t index,
                           const BackgroundChange& change) {
  out.u32(layer);
  out.u32(index);
  out.f32(change.m_fStartBeat);
  out.f32(change.m_fRate);
  out.string(change.m_def.m_sEffect);
  out.string(change.m_def.m_sFile1);
  out.string(change.m_def.m_sFile2);
  out.string(change.m_def.m_sColor1);
  out.string(change.m_def.m_sColor2);
  out.string(change.m_sTransition);
}

ItgOracleBuffer error_buffer(const std::string& message) {
  Writer out;
  out.header(1);
  out.string(message);
  return out.release();
}

}  // namespace

extern "C" ItgOracleBuffer itg_oracle_load_charts(const uint8_t* path,
    size_t path_len, const uint8_t* theme, size_t theme_len,
    const uint8_t* host, size_t host_len) {
  try {
    const std::lock_guard<std::mutex> guard(harness_native_mutex());
    if (path == nullptr) {
      return error_buffer("simfile path pointer is null");
    }
    const std::string simfile(reinterpret_cast<const char*>(path), path_len);
    const std::string theme_path(reinterpret_cast<const char*>(theme), theme_len);
    const std::string theme_host(reinterpret_cast<const char*>(host), host_len);
    harness_clear_diagnostics();
    Song song;
    if (!load_song(simfile, song)) {
      return error_buffer("ITGmania failed to load the simfile");
    }

    Writer out;
    out.header(0);
    const std::vector<Steps*>& charts = song.GetAllSteps();
    if (charts.empty()) return error_buffer("ITGmania loaded no charts");
    out.u32(wire_len(charts.size(), "chart count"));
    for (size_t index = 0; index < charts.size(); ++index) {
      Steps* chart = charts[index];
      out.u32(wire_len(index, "chart index"));
      write_chart(out, *chart, song);
      if (theme_path.empty()) {
        out.string("");
      } else if (chart->m_StepsType == StepsType_Invalid) {
        out.string(R"({"status":"unavailable","errors":["native steps type is unsupported"]})");
      } else {
        // SL selects non-Edit charts by type/difficulty and Edits by description.
        // Keep its output, but never certify a repeated first match as a baseline.
        bool ambiguous = false;
        for (Steps* other : charts) {
          if (other != chart && other->m_StepsType == chart->m_StepsType &&
              other->GetDifficulty() == chart->GetDifficulty() &&
              (chart->GetDifficulty() != Difficulty_Edit ||
               other->GetDescription() == chart->GetDescription())) ambiguous = true;
        }
        out.string(harness_chart_theme(*chart, theme_path, theme_host, ambiguous));
      }
    }
    const auto diagnostics = harness_take_diagnostics();
    out.u32(wire_len(diagnostics.size(), "diagnostic count"));
    for (const auto& diagnostic : diagnostics) out.string(diagnostic);
    return out.release();
  } catch (const std::exception& error) {
    try {
      return error_buffer(error.what());
    } catch (...) {
      return {nullptr, 0};
    }
  } catch (...) {
    try {
      return error_buffer("unknown native oracle failure");
    } catch (...) {
      return {nullptr, 0};
    }
  }
}

extern "C" ItgOracleBuffer itg_oracle_load_song_lua(const uint8_t* path,
                                                      size_t path_len) {
  try {
    const std::lock_guard<std::mutex> guard(harness_native_mutex());
    if (path == nullptr) {
      return song_lua_error_buffer("simfile path pointer is null");
    }
    const std::string simfile(reinterpret_cast<const char*>(path), path_len);
    Song song;
    if (!load_song(simfile, song)) {
      return song_lua_error_buffer("ITGmania failed to load the simfile");
    }

    const auto& background_1 =
        song.GetBackgroundChanges(BACKGROUND_LAYER_1);
    const auto& background_2 =
        song.GetBackgroundChanges(BACKGROUND_LAYER_2);
    const auto& foreground = song.GetForegroundChanges();
    const size_t change_count =
        background_1.size() + background_2.size() + foreground.size();

    Writer out;
    out.song_lua_header(0);
    out.string(song.m_sSongFileName);
    out.string(song.GetSongDir());
    out.string(song.m_sMainTitle);
    song.m_SongTiming.TidyUpData(false);
    out.f32(song.GetSpecifiedLastSecond());
    out.f32(song.GetSpecifiedLastBeat());
    out.u32(wire_len(change_count, "song Lua change count"));
    for (size_t index = 0; index < background_1.size(); ++index) {
      write_song_lua_change(out, 0, wire_len(index, "background index"),
                            background_1[index]);
    }
    for (size_t index = 0; index < background_2.size(); ++index) {
      write_song_lua_change(out, 1, wire_len(index, "background index"),
                            background_2[index]);
    }
    for (size_t index = 0; index < foreground.size(); ++index) {
      write_song_lua_change(out, 2, wire_len(index, "foreground index"),
                            foreground[index]);
    }
    return out.release();
  } catch (const std::exception& error) {
    try {
      return song_lua_error_buffer(error.what());
    } catch (...) {
      return {nullptr, 0};
    }
  } catch (...) {
    try {
      return song_lua_error_buffer("unknown native song Lua oracle failure");
    } catch (...) {
      return {nullptr, 0};
    }
  }
}

extern "C" ItgOracleBuffer itg_oracle_eval_song_lua(
    const uint8_t* request_data, size_t request_len, const uint8_t* host,
    size_t host_len) {
  const std::lock_guard<std::mutex> guard(harness_native_mutex());
  lua_State* state = nullptr;
  try {
    if (request_data == nullptr || host == nullptr) {
      return song_lua_semantic_error_buffer(
          "semantic request and host pointers must not be null");
    }
    const SongLuaSemanticRequest request =
        read_song_lua_semantic_request(request_data, request_len);
    if (request.entries.empty()) {
      return song_lua_semantic_error_buffer(
          "semantic request has no Lua entries");
    }
    if (!std::isfinite(request.max_beat) || request.max_beat < 0.0f ||
        !std::isfinite(request.bpm) || request.bpm <= 0.0f ||
        !std::isfinite(request.beat_step) || request.beat_step <= 0.0f) {
      return song_lua_semantic_error_buffer(
          "semantic timing values must be finite and positive");
    }

    state = luaL_newstate();
    if (state == nullptr) {
      return song_lua_semantic_error_buffer("could not create Lua state");
    }
    luaL_openlibs(state);
    harness_register_lua_globals(state);
    push_song_lua_semantic_request(state, request);
    // Stream the final document out of Lua. Whole-song traces otherwise keep
    // nested JSON strings alongside every retained observation and its copies.
    std::string document;
    lua_pushlightuserdata(state, &document);
    lua_pushcclosure(state, [](lua_State* L) -> int {
      auto* output = static_cast<std::string*>(lua_touserdata(L, lua_upvalueindex(1)));
      size_t length = 0;
      const char* chunk = luaL_checklstring(L, 1, &length);
      bool failed = false;
      try {
        output->append(chunk, length);
      } catch (...) {
        failed = true;
      }
      if (failed) return luaL_error(L, "could not allocate semantic JSON buffer");
      return 0;
    }, 1);
    lua_setglobal(state, "_ITG_JSON_APPEND");
    lua_pushcfunction(state, [](lua_State* L) -> int {
      const StepsType type = Enum::Check<StepsType>(L, 1);
      const std::string name = "StepsType_" + StepsTypeToString(type);
      lua_pushlstring(L, name.data(), name.size());
      return 1;
    });
    lua_setglobal(state, "_ITG_STEPS_TYPE");
    lua_pushcfunction(state, [](lua_State* L) -> int {
      song_lua_frame_beat = luaL_checknumber(L, 1);
      // The initial zero-delta update can build chart-sized spline data.
      // Charge it to the bounded loading quota; later frames keep their own cap.
      if (song_lua_first_frame) {
        song_lua_first_frame = false;
      } else {
        song_lua_instruction_budget = SONG_LUA_INSTRUCTION_LIMIT;
      }
      return 0;
    });
    lua_setglobal(state, "_ITG_BEGIN_FRAME");
    // PlayerOptions::FromOneModString uses NOTESKIN for clearall and skin
    // validation. Keep its native manager alive for the whole Lua session.
    NoteSkinManager option_skins;
    NoteSkinGlobalGuard option_skin_global(&option_skins);
    install_option_queries(state);
    install_actor_math(state);
    // Actor's Lua color methods share this parser, including legacy RGBA
    // arguments, raw table reads and conversion to the engine's float type.
    lua_pushcfunction(state, [](lua_State* L) -> int {
      RageColor color;
      color.FromStackCompat(L, 1);
      color.PushTable(L);
      return 1;
    });
    lua_setglobal(state, "_ITG_COLOR");

    // Keep the parsed chart alive until Lua closes. Use ITGmania's timing
    // implementation so the oracle includes authored SCROLLS and SPEEDS.
    Song timing_song;
    std::string timing_extension = std::filesystem::path(request.simfile).extension().string();
    for (char& ch : timing_extension) ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
    const bool has_simfile = timing_extension == ".sm" || timing_extension == ".sma" ||
                             timing_extension == ".ssc" || timing_extension == ".ats";
    if (has_simfile && std::filesystem::is_regular_file(request.simfile)) {
      if (!load_song(request.simfile, timing_song)) {
        throw std::runtime_error("could not load semantic chart timing");
      }
      lua_getglobal(state, "_HARNESS");
      const std::string background = timing_song.GetBackgroundPath();
      if (!background.empty()) lua_field(state, "background_path", background);
      lua_pop(state, 1);
      Steps* selected = nullptr;
      for (Steps* steps : timing_song.GetAllSteps()) {
        if (steps->m_StepsTypeStr == request.steps_type &&
            steps->GetDifficulty() == StringToDifficulty(request.difficulty.substr(11)) &&
            steps->GetDescription() == request.description) {
          selected = steps;
          break;
        }
      }
      if (selected == nullptr) {
        throw std::runtime_error("could not select semantic chart timing: " +
                                 request.steps_type + ", " + request.difficulty + ", " + request.description);
      }
      // Song::GetAllSteps exposes the native load order. Share each Steps
      // object with GetCurrentSteps so identity comparisons find its index.
      lua_getglobal(state, "_HARNESS");
      const auto& all_steps = timing_song.GetAllSteps();
      uint32_t current_steps = 0;
      lua_createtable(state, static_cast<int>(all_steps.size()), 0);
      for (size_t index = 0; index < all_steps.size(); ++index) {
        const Steps* steps = all_steps[index];
        lua_newtable(state);
        lua_field(state, "difficulty", "Difficulty_" + DifficultyToString(steps->GetDifficulty()));
        lua_field(state, "steps_type", "StepsType_" + StepsTypeToString(steps->m_StepsType));
        lua_field(state, "description", steps->GetDescription());
        lua_pushinteger(state, steps->GetMeter());
        lua_setfield(state, -2, "meter");
        lua_rawseti(state, -2, static_cast<int>(index + 1));
        if (steps == selected) {
          current_steps = static_cast<uint32_t>(index + 1);
        }
      }
      lua_setfield(state, -2, "steps");
      lua_field(state, "current_steps", current_steps);
      lua_pop(state, 1);
      TimingData* timing = selected->GetTimingData();
      timing->TidyUpData(false);
      timing->PrepareLookup();
      lua_pushlightuserdata(state, timing);
      lua_pushcclosure(state, [](lua_State* L) -> int {
        auto* timing = static_cast<TimingData*>(lua_touserdata(L, lua_upvalueindex(1)));
        const float note = static_cast<float>(luaL_checknumber(L, 1));
        const float beat = static_cast<float>(luaL_checknumber(L, 2));
        const float seconds = static_cast<float>(luaL_checknumber(L, 3)) + timing->GetElapsedTimeFromBeat(0.0f);
        const float offset = (timing->GetDisplayedBeat(note) - timing->GetDisplayedBeat(beat)) *
            timing->GetDisplayedSpeedPercent(beat, seconds);
        lua_pushnumber(L, offset * 64.0f);
        return 1;
      }, 1);
      lua_setglobal(state, "_ITG_TIMING_Y_OFFSET");
      // Continuous BPM maps retain the existing double-precision clock. Pauses
      // and warps must use the song's native timing, independently of Steps.
      TimingData* song_timing = &timing_song.m_SongTiming;
      song_timing->TidyUpData(false);
      song_timing->PrepareLookup();
      if (song_timing->HasStops() || song_timing->HasDelays() || song_timing->HasWarps()) {
        lua_pushlightuserdata(state, song_timing);
        lua_pushcclosure(state, [](lua_State* L) -> int {
          auto* timing = static_cast<TimingData*>(lua_touserdata(L, lua_upvalueindex(1)));
          TimingData::GetBeatArgs args;
          // Traces use seconds relative to beat zero, as the continuous clock
          // does. Restore the native music timestamp before asking TimingData.
          args.elapsed_time = static_cast<float>(luaL_checknumber(L, 1)) +
                              timing->GetElapsedTimeFromBeat(0.0f);
          timing->GetBeatAndBPSFromElapsedTimeNoOffset(args);
          lua_pushnumber(L, args.beat);
          lua_pushnumber(L, args.bps_out);
          lua_pushboolean(L, args.freeze_out);
          lua_pushboolean(L, args.delay_out);
          return 4;
        }, 1);
        lua_setglobal(state, "_ITG_SONG_POSITION");
        lua_pushlightuserdata(state, song_timing);
        lua_pushcclosure(state, [](lua_State* L) -> int {
          auto* timing = static_cast<TimingData*>(lua_touserdata(L, lua_upvalueindex(1)));
          const float beat = static_cast<float>(luaL_checknumber(L, 1));
          lua_pushnumber(L, timing->GetElapsedTimeFromBeat(beat) - timing->GetElapsedTimeFromBeat(0.0f));
          return 1;
        }, 1);
        lua_setglobal(state, "_ITG_SONG_SECONDS");
      }
    }

    lua_getglobal(state, "os");
    if (lua_istable(state, -1)) {
      lua_pushnil(state);
      lua_setfield(state, -2, "execute");
      lua_pushnil(state);
      lua_setfield(state, -2, "remove");
      lua_pushnil(state);
      lua_setfield(state, -2, "rename");
    }
    lua_pop(state, 1);
    lua_getglobal(state, "io");
    if (lua_istable(state, -1)) {
      lua_pushnil(state);
      lua_setfield(state, -2, "popen");
    }
    lua_pop(state, 1);
    lua_getglobal(state, "package");
    if (lua_istable(state, -1)) {
      lua_pushnil(state);
      lua_setfield(state, -2, "loadlib");
    }
    lua_pop(state, 1);

    const int64_t semantic_ticks =
        static_cast<int64_t>(std::ceil(request.max_beat / request.beat_step)) +
        1;
    // Retain the existing loading quota, then bound each 60 Hz replay frame
    // separately. A whole-song quota also charges finite host bookkeeping and
    // rejects long replays, especially when render samples are downsampled.
    song_lua_frame_beat = 0;
    song_lua_first_frame = true;
    song_lua_instruction_budget = std::clamp<int64_t>(
        semantic_ticks * 2000000, SONG_LUA_INSTRUCTION_LIMIT, 20000000000);
    lua_sethook(state, song_lua_instruction_hook, LUA_MASKCOUNT, 10000);
    const int load_status = luaL_loadbuffer(
        state, reinterpret_cast<const char*>(host), host_len,
        "@semantic_host.lua");
    if (load_status != 0) {
      const std::string message = lua_error_text(state);
      lua_close(state);
      state = nullptr;
      return song_lua_semantic_error_buffer(message);
    }
    const int call_status = lua_pcall(state, 0, 1, 0);
    if (call_status != 0) {
      const std::string message = lua_error_text(state);
      lua_close(state);
      state = nullptr;
      return song_lua_semantic_error_buffer(message);
    }
    size_t json_len = 0;
    const char* json = lua_tolstring(state, -1, &json_len);
    if (json == nullptr) {
      lua_close(state);
      state = nullptr;
      return song_lua_semantic_error_buffer(
          "semantic host did not return JSON text");
    }
    // Custom hosts may still return their JSON directly.
    if (document.empty()) document.assign(json, json_len);
    lua_close(state);
    state = nullptr;
    Writer out;
    out.song_lua_semantic_header(0);
    out.string(document);
    return out.release();
  } catch (const std::exception& error) {
    if (state != nullptr) lua_close(state);
    try {
      return song_lua_semantic_error_buffer(error.what());
    } catch (...) {
      return {nullptr, 0};
    }
  } catch (...) {
    if (state != nullptr) lua_close(state);
    try {
      return song_lua_semantic_error_buffer(
          "unknown native song Lua semantic oracle failure");
    } catch (...) {
      return {nullptr, 0};
    }
  }
}

extern "C" ItgOracleBuffer itg_oracle_load_font(
    const uint8_t* path, size_t path_len, const uint8_t* text,
    size_t text_len, uint8_t mapped_only,
    const uint8_t* source_root, size_t source_root_len) {
  try {
    const std::lock_guard<std::mutex> guard(harness_native_mutex());
    if (path == nullptr || (mapped_only == 0 && text == nullptr)) {
      return font_error_buffer("font path and text pointers must not be null");
    }
    std::filesystem::path font_path(std::string(
        reinterpret_cast<const char*>(path), path_len));
    std::error_code path_error;
    font_path = std::filesystem::canonical(font_path, path_error);
    if (path_error) {
      return font_error_buffer("could not open font path: " +
                               path_error.message());
    }
    const std::filesystem::path theme_fonts = find_fonts_root(font_path);
    const std::filesystem::path themes_dir = theme_fonts.parent_path().parent_path();
    std::filesystem::path fallback_fonts = themes_dir / "_fallback" / "Fonts";
    // A separately pinned theme can use the engine checkout's fallback fonts.
    // An adjacent fallback remains authoritative for complete theme installs.
    if (!std::filesystem::is_directory(fallback_fonts) && source_root != nullptr) {
      fallback_fonts = std::filesystem::path(std::string(
          reinterpret_cast<const char*>(source_root), source_root_len)) /
          "Themes" / "_fallback" / "Fonts";
    }
    harness_configure_font_paths(theme_fonts.generic_string(),
                                 fallback_fonts.generic_string());
    harness_clear_diagnostics();

    std::string extension = font_path.extension().string();
    for (char& ch : extension) {
      ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
    }
    if (extension == ".redir") {
      std::string redirect;
      if (!GetFileContents(font_path.generic_string(), redirect, true) ||
          redirect.empty()) {
        return font_error_buffer("font redirect is empty");
      }
      const std::string resolved = THEME->GetPathF("", redirect, true);
      if (resolved.empty()) {
        return font_error_buffer("font redirect target was not found: " + redirect);
      }
      font_path = std::filesystem::canonical(resolved, path_error);
      if (path_error) {
        return font_error_buffer("could not resolve font redirect: " +
                                 path_error.message());
      }
      extension = font_path.extension().string();
      for (char& ch : extension) {
        ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
      }
    }
    if (extension != ".ini") {
      return font_error_buffer("font path must resolve to an .ini file");
    }

    Font font;
    font.Load(font_path.generic_string(), "");
    const std::wstring wide_text = mapped_only != 0
                                       ? mapped_font_text(font)
                                       : RStringToWstring(std::string(
                                             reinterpret_cast<const char*>(text),
                                             text_len));
    const std::string utf8_text = WStringToRString(wide_text);

    std::vector<const FontPage*> pages;
    std::map<const FontPage*, uint32_t> page_indices;
    std::vector<ProbedGlyph> glyphs;
    glyphs.reserve(wide_text.size());
    int32_t pen_x = 0;
    for (wchar_t codepoint : wide_text) {
      const std::wstring one(1, codepoint);
      const bool available = font.FontCompleteForString(one);
      const glyph& value = font.GetGlyph(codepoint);
      auto [page_it, inserted] = page_indices.emplace(
          value.m_pPage, wire_len(pages.size(), "observed font page count"));
      if (inserted) pages.push_back(value.m_pPage);
      glyphs.push_back({codepoint, &value, available, page_it->second, pen_x});
      pen_x += value.m_iHadvance;
    }

    Writer out;
    out.font_header(0);
    out.string(font_path.generic_string());
    out.string(theme_fonts.generic_string());
    out.string(fallback_fonts.generic_string());
    out.string("headless_png_metadata");
    out.string(utf8_text);
    out.i32(font.GetHeight());
    out.i32(font.GetLineSpacing());
    out.byte(font.IsRightToLeft() ? 1 : 0);
    out.byte(font.IsDistanceField() ? 1 : 0);
    const RageColor& stroke_color = font.GetDefaultStrokeColor();
    out.f32(stroke_color.r);
    out.f32(stroke_color.g);
    out.f32(stroke_color.b);
    out.f32(stroke_color.a);
    out.i32(font.GetLineWidthInSourcePixels(wide_text));
    out.i32(font.GetLineHeightInSourcePixels(wide_text));
    out.u32(wire_len(pages.size(), "observed font page count"));
    for (const FontPage* page : pages) write_font_page(out, *page);
    out.u32(wire_len(glyphs.size(), "font glyph count"));
    for (const ProbedGlyph& probe : glyphs) write_probed_glyph(out, probe);
    const std::vector<std::string> diagnostics = harness_take_diagnostics();
    out.u32(wire_len(diagnostics.size(), "font diagnostic count"));
    for (const std::string& diagnostic : diagnostics) out.string(diagnostic);
    return out.release();
  } catch (const std::exception& error) {
    try {
      return font_error_buffer(error.what());
    } catch (...) {
      return {nullptr, 0};
    }
  } catch (...) {
    try {
      return font_error_buffer("unknown native font oracle failure");
    } catch (...) {
      return {nullptr, 0};
    }
  }
}

extern "C" ItgOracleBuffer itg_oracle_load_noteskin(
    const uint8_t* root, size_t root_len, const uint8_t* game,
    size_t game_len, const uint8_t* skin, size_t skin_len,
    const uint8_t* request, size_t request_len) {
  try {
    const std::lock_guard<std::mutex> guard(harness_native_mutex());
    if (root == nullptr || game == nullptr || skin == nullptr ||
        request == nullptr) {
      return noteskin_error_buffer("noteskin oracle pointers must not be null");
    }
    std::filesystem::path noteskins_root(std::string(
        reinterpret_cast<const char*>(root), root_len));
    std::error_code path_error;
    noteskins_root = std::filesystem::canonical(noteskins_root, path_error);
    if (path_error || !std::filesystem::is_directory(noteskins_root)) {
      return noteskin_error_buffer("could not open NoteSkins directory");
    }
    std::string root_name = noteskins_root.filename().string();
    for (char& ch : root_name) {
      ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
    }
    if (root_name != "noteskins") {
      return noteskin_error_buffer("noteskin root must be named NoteSkins");
    }

    const std::string game_name(reinterpret_cast<const char*>(game), game_len);
    const std::string skin_name(reinterpret_cast<const char*>(skin), skin_len);
    const NoteSkinRequest parsed = read_noteskin_request(request, request_len);
    CurrentPathGuard current_path(noteskins_root.parent_path());
    harness_clear_diagnostics();

    Game native_game{};
    native_game.m_szName = game_name.c_str();
    NoteSkinManager manager;
    NoteSkinGlobalGuard global(&manager);
    manager.RefreshNoteSkinData(&native_game);
    const std::vector<std::string> load_diagnostics =
        harness_take_diagnostics();

    std::vector<std::string> skins;
    manager.GetNoteSkinNames(&native_game, skins, true);
    if (!manager.NoteSkinNameInList(skin_name, skins)) {
      return noteskin_error_buffer("noteskin was not loaded for game " +
                                   game_name + ": " + skin_name);
    }
    manager.SetCurrentNoteSkin(skin_name);

    Writer out;
    out.noteskin_header(0);
    out.string(noteskins_root.generic_string());
    out.string(game_name);
    out.string(skin_name);
    out.u32(wire_len(skins.size(), "noteskin inventory count"));
    for (const std::string& name : skins) out.string(name);

    out.u32(wire_len(parsed.metrics.size(), "noteskin metric count"));
    for (const MetricRequest& metric : parsed.metrics) {
      out.string(metric.section);
      out.string(metric.key);
      out.string(manager.GetMetric(metric.section, metric.key));
      out.i32(manager.GetMetricI(metric.section, metric.key));
      out.f32(manager.GetMetricF(metric.section, metric.key));
      out.byte(manager.GetMetricB(metric.section, metric.key) ? 1 : 0);
    }

    out.u32(wire_len(parsed.paths.size(), "noteskin path count"));
    for (const PathRequest& path : parsed.paths) {
      out.string(path.button);
      out.string(path.element);
      out.string(manager.GetPath(path.button, path.element));
    }
    harness_clear_diagnostics();
    out.u32(wire_len(load_diagnostics.size(), "noteskin diagnostic count"));
    for (const std::string& diagnostic : load_diagnostics) {
      out.string(diagnostic);
    }
    return out.release();
  } catch (const std::exception& error) {
    try {
      return noteskin_error_buffer(error.what());
    } catch (...) {
      return {nullptr, 0};
    }
  } catch (...) {
    try {
      return noteskin_error_buffer("unknown native noteskin oracle failure");
    } catch (...) {
      return {nullptr, 0};
    }
  }
}

extern "C" void itg_oracle_free(uint8_t* data) { delete[] data; }
