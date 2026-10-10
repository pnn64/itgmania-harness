// Include the native implementations so queries use their actual Lua setters,
// FromString parsers and equality operators, without changing reference sources.
#include "PlayerOptions.cpp"
#define AddPart SongOptionsAddPart
#include "SongOptions.cpp"
#undef AddPart
#include <new>
#include <limits>
#include <memory>
#include "NoteSkinManager.h"
#include "ModsGroup.cpp"
#include "runtime_stubs.h"

namespace {
struct OptionClock {
  int64_t previous = harness_native_time();
  OptionClock() { harness_native_time(0); }
  ~OptionClock() { harness_native_time(previous); }
};

struct OptionState {
  OptionClock clock;
  ModsGroup<PlayerOptions> players[2];
  ModsGroup<SongOptions> saved_song = GAMESTATE->m_SongOptions;
  int64_t current_time = 0;
  bool saved_rate_pref = PREFSMAN->m_bRateModsAffectTweens;
  NoteSkinManager* saved_skin = NOTESKIN;
  std::unique_ptr<NoteSkinManager> fallback_skin;
  OptionState() {
    // FromOneModString requires a manager even for numeric modifiers. Song
    // captures provide their populated manager; standalone numeric queries
    // use an empty native manager without inventing skin assets.
    if (!NOTESKIN) {
      fallback_skin = std::make_unique<NoteSkinManager>();
      NOTESKIN = fallback_skin.get();
    }
    GAMESTATE->m_SongOptions = ModsGroup<SongOptions>{};
  }
  ~OptionState() {
    GAMESTATE->m_SongOptions = saved_song;
    PREFSMAN->m_bRateModsAffectTweens.Set(saved_rate_pref);
    if (fallback_skin) NOTESKIN = saved_skin;
  }
};

template<class Owner> struct OptionMethod {
  const char* name;
  int (*call)(Owner*, lua_State*);
};

const OptionMethod<PlayerOptions> player_methods[] = {
#include "player_option_methods.inc"
};
const OptionMethod<SongOptions> song_methods[] = {
#define SONG_METHOD(name) {#name, &LunaSongOptions::name}
  SONG_METHOD(AutosyncSetting), SONG_METHOD(AssistClap),
  SONG_METHOD(AssistMetronome), SONG_METHOD(StaticBackground),
  SONG_METHOD(RandomBGOnly), SONG_METHOD(SaveScore), SONG_METHOD(SaveReplay),
  SONG_METHOD(MusicRate), SONG_METHOD(Haste),
#undef SONG_METHOD
};

template<class Owner, size_t N>
int call_option(Owner* owner, lua_State* L, const std::string& name,
                const OptionMethod<Owner> (&methods)[N]) {
  for (const auto& method : methods) {
    if (name == method.name) return method.call(owner, L);
  }
  return luaL_error(L, "unavailable native option method %s", name.c_str());
}

int option_call(OptionState* state, int player, ModsLevel level,
                const std::string& method, lua_State* L) {
  if (player == -1) {
    auto& song = GAMESTATE->m_SongOptions;
    if (method == "GetString") { LuaHelpers::Push(L, song.Get(level).GetString()); return 1; }
    if (method == "SetSongOptions") {
      SongOptions options;
      options.FromString(luaL_checkstring(L, 1));
      song.Assign(level, options);
      return 0;
    }
    return call_option(&song.Get(level), L, method, song_methods);
  }
  if (player < 0 || player > 1) return luaL_error(L, "invalid player");
  if (method == "GetString") {
    LuaHelpers::Push(L, state->players[player].Get(level).GetString());
    return 1;
  }
  if (method == "GetMods") {
    std::vector<std::string> parts;
    state->players[player].Get(level).GetMods(parts);
    LuaHelpers::CreateTableFromArray(parts, L);
    return 1;
  }
  if (method == "SetPlayerOptions") {
    // LunaPlayerState::SetPlayerOptions assigns freshly parsed options; it
    // does not add modifiers to the previous PlayerOptions instance.
    PlayerOptions options;
    options.FromString(luaL_checkstring(L, 1));
    state->players[player].Assign(level, options);
    return 0;
  }
  // OPTIONAL_RETURN_SELF tests original_top. Index zero is not a valid Lua
  // argument; use the equivalent explicit-nil native getter to avoid reading
  // a stale slot outside the argument stack after a successful skin setter.
  if (method == "NoteSkin" && lua_gettop(L) == 0) lua_pushnil(L);
  return call_option(&state->players[player].Get(level), L, method, player_methods);
}

int update_options(lua_State* L) {
  auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
  const int player = static_cast<int>(luaL_checkinteger(L, 1));
  const std::string method = luaL_checkstring(L, 2);
  lua_remove(L, 1); lua_remove(L, 1);
  return option_call(state, player, ModsLevel_Song, method, L);
}

int options_at_level(lua_State* L) {
  auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
  const int player = static_cast<int>(luaL_checkinteger(L, 1));
  const ModsLevel level = Enum::Check<ModsLevel>(L, 2);
  const std::string method = luaL_checkstring(L, 3);
  lua_remove(L, 1); lua_remove(L, 1); lua_remove(L, 1);
  return option_call(state, player, level, method, L);
}

int using_modifier(lua_State* L) {
  auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
  const int player = static_cast<int>(luaL_checkinteger(L, 1));
  if (player < 0 || player > 1) return luaL_error(L, "invalid player");
  const std::string text = luaL_checkstring(L, 2);
  // GameState::PlayerIsUsingModifier: apply to copies and compare current values.
  PlayerOptions po = state->players[player].GetCurrent();
  SongOptions so = GAMESTATE->m_SongOptions.GetCurrent();
  po.FromString(text);
  so.FromString(text);
  lua_pushboolean(L, po == state->players[player].GetCurrent() && so == GAMESTATE->m_SongOptions.GetCurrent());
  return 1;
}
}

void install_option_queries(lua_State* L) {
  // Native LuaXType builds both directions used by ENUM_INTERFACE.
  LuaLifeType(L);
  LuaDrainType(L);
  LuaHideLightType(L);
  LuaModTimerType(L);
  LuaModsLevel(L);
  new (lua_newuserdata(L, sizeof(OptionState))) OptionState;
  lua_newtable(L);
  lua_pushcfunction(L, [](lua_State* L) -> int {
    static_cast<OptionState*>(lua_touserdata(L, 1))->~OptionState();
    return 0;
  });
  lua_setfield(L, -2, "__gc");
  lua_setmetatable(L, -2);
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, update_options, 1);
  lua_setglobal(L, "_ITG_OPTIONS_UPDATE");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, options_at_level, 1);
  lua_setglobal(L, "_ITG_OPTIONS_AT");
  lua_pushcfunction(L, [](lua_State* L) -> int {
    LuaHelpers::Push(L, "ModsLevel_" + ModsLevelToString(Enum::Check<ModsLevel>(L, 1)));
    return 1;
  });
  lua_setglobal(L, "_ITG_OPTIONS_LEVEL");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const double seconds = luaL_checknumber(L, 1);
    if (!std::isfinite(seconds) || seconds < 0 ||
        seconds >= static_cast<double>(std::numeric_limits<int64_t>::max()) / 1000000.0)
      return luaL_error(L, "invalid native option clock");
    const int64_t time = std::llround(seconds * 1000000);
    if (time < state->current_time) return luaL_error(L, "native option clock moved backwards");
    state->current_time = time;
    harness_native_time(time);
    // GameState::Update advances SongOptions before PlayerState::Update.
    // ModsGroup replaces the supplied frame delta with the unscaled RageTimer.
    const float delta = lua_gettop(L) >= 2 ? FArg(2) : 0.0f;
    GAMESTATE->m_SongOptions.Update(delta);
    for (auto& player : state->players) player.Update(delta);
    return 0;
  }, 1);
  lua_setglobal(L, "_ITG_OPTIONS_ADVANCE");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    state->players[player].Assign(ModsLevel_Preferred, state->players[player].GetSong());
    return 0;
  }, 1);
  lua_setglobal(L, "_ITG_OPTIONS_SEED");
  lua_pushcfunction(L, [](lua_State* L) -> int {
    lua_pushboolean(L, PREFSMAN->m_bRateModsAffectTweens);
    if (lua_isboolean(L, 1)) PREFSMAN->m_bRateModsAffectTweens.Set(lua_toboolean(L, 1) != 0);
    return 1;
  });
  lua_setglobal(L, "_ITG_OPTIONS_RATE_TWEENS");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    const ModsLevel level = lua_gettop(L) >= 3 ? Enum::Check<ModsLevel>(L, 3) : ModsLevel_Song;
    std::vector<std::string> parts;
    split(luaL_checkstring(L, 2), ",", parts, true);
    lua_newtable(L);
    int index = 0;
    for (std::string part : parts) {
      Trim(part);
      std::string lower = part;
      MakeLower(lower);
      std::vector<std::string> words;
      split(lower, " ", words, true);
      if (words.empty()) continue;
      const std::string& key = words.back();
      // Unrelated tokens need no skin probe. In particular, do not execute
      // ChooseRandomModifiers a second time through a query on a copy.
      const bool named = NOTESKIN->DoesNoteSkinExist(key);
      if (key == "random" && !named) continue;
      if (key != "clearall" && key != "noteskin" && !named &&
          !NOTESKIN->DoesNoteSkinExist(lower)) continue;
      PlayerOptions probe = state->players[player].Get(level);
      // A copy with an impossible skin distinguishes the native skin branch
      // from a same-name numeric modifier without mutating live options.
      probe.m_sNoteSkin = "\1harness-skin-probe";
      probe.FromString(part);
      if (probe.m_sNoteSkin == "\1harness-skin-probe") continue;
      lua_newtable(L);
      LuaHelpers::Push(L, part); lua_setfield(L, -2, "part");
      LuaHelpers::Push(L, probe.m_sNoteSkin); lua_setfield(L, -2, "target");
      lua_rawseti(L, -2, ++index);
    }
    return 1;
  }, 1);
  lua_setglobal(L, "_ITG_OPTIONS_SKINS");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    const ModsLevel level = lua_gettop(L) >= 3 ? Enum::Check<ModsLevel>(L, 3) : ModsLevel_Song;
    std::vector<std::string> parts;
    split(luaL_checkstring(L, 2), ",", parts, true);
    lua_newtable(L);
    int index = 0;
    for (std::string part : parts) {
      Trim(part);
      if (part.empty()) continue;
      std::string lower = part;
      MakeLower(lower);
      std::vector<std::string> words;
      split(lower, " ", words, true);
      if (words.empty()) continue;
      const std::string& key = words.back();
      // Do not consume RNG again or classify native skin branches as errors.
      if (key == "random" || NOTESKIN->DoesNoteSkinExist(key) ||
          NOTESKIN->DoesNoteSkinExist(lower)) continue;
      const PlayerOptions& current = state->players[player].Get(level);
      PlayerOptions probe = current;
      std::string error;
      if (probe.FromOneModString(part, error)) continue;
      lua_newtable(L);
      LuaHelpers::Push(L, part); lua_setfield(L, -2, "part");
      LuaHelpers::Push(L, key); lua_setfield(L, -2, "key");
      lua_pushboolean(L, false); lua_setfield(L, -2, "accepted");
      lua_pushboolean(L, probe == current); lua_setfield(L, -2, "unchanged");
      LuaHelpers::Push(L, error); lua_setfield(L, -2, "error");
      lua_rawseti(L, -2, ++index);
    }
    return 1;
  }, 1);
  lua_setglobal(L, "_ITG_OPTIONS_REJECTED");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    const std::string modifiers = luaL_checkstring(L, 2);
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    // GameState::ApplyStageModifiers calls the native ModsGroup at Stage.
    state->players[player].FromString(ModsLevel_Stage, modifiers);
    GAMESTATE->m_SongOptions.FromString(ModsLevel_Stage, modifiers);
    return 0;
  }, 1);
  lua_setglobal(L, "_ITG_APPLY_STAGE_MODIFIERS");
  lua_pushcclosure(L, using_modifier, 1);
  lua_setglobal(L, "_ITG_USING_MODIFIER");
}

// Native assertion controls use the manager's persistent Lua state. Release
// their query closures and restore the clock/GameState before the next oracle.
void clear_option_queries(lua_State* L) {
  for (const char* name : {"_ITG_OPTIONS_UPDATE", "_ITG_OPTIONS_AT", "_ITG_OPTIONS_LEVEL",
      "_ITG_OPTIONS_ADVANCE", "_ITG_OPTIONS_SEED", "_ITG_OPTIONS_RATE_TWEENS",
      "_ITG_OPTIONS_SKINS", "_ITG_OPTIONS_REJECTED", "_ITG_APPLY_STAGE_MODIFIERS",
      "_ITG_USING_MODIFIER"}) {
    lua_pushnil(L); lua_setglobal(L, name);
  }
  lua_gc(L, LUA_GCCOLLECT, 0);
}
