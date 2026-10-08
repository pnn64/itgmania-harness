// Include the native implementations so queries use their actual Lua setters,
// FromString parsers and equality operators, without changing reference sources.
#include "PlayerOptions.cpp"
#define AddPart SongOptionsAddPart
#include "SongOptions.cpp"
#undef AddPart
#include <new>
#include "ModsGroup.h"

namespace {
struct OptionState {
  ModsGroup<PlayerOptions> players[2];
  ModsGroup<SongOptions> song;
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

int update_options(lua_State* L) {
  auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
  const int player = static_cast<int>(luaL_checkinteger(L, 1));
  const std::string method = luaL_checkstring(L, 2);
  lua_remove(L, 1);
  lua_remove(L, 1);
  if (player == -1) {
    if (method == "GetString") { LuaHelpers::Push(L, state->song.GetSong().GetString()); return 1; }
    return call_option(&state->song.GetSong(), L, method, song_methods);
  }
  if (player < 0 || player > 1) return luaL_error(L, "invalid player");
  if (method == "GetString") {
    LuaHelpers::Push(L, state->players[player].GetSong().GetString());
    return 1;
  }
  if (method == "SetPlayerOptions") {
    // LunaPlayerState::SetPlayerOptions assigns freshly parsed options; it
    // does not add modifiers to the previous PlayerOptions instance.
    PlayerOptions options;
    options.FromString(luaL_checkstring(L, 1));
    state->players[player].Assign(ModsLevel_Song, options);
    return 0;
  }
  // OPTIONAL_RETURN_SELF tests original_top. Index zero is not a valid Lua
  // argument; use the equivalent explicit-nil native getter to avoid reading
  // a stale slot outside the argument stack after a successful skin setter.
  if (method == "NoteSkin" && lua_gettop(L) == 0) lua_pushnil(L);
  return call_option(&state->players[player].GetSong(), L, method, player_methods);
}

int using_modifier(lua_State* L) {
  auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
  const int player = static_cast<int>(luaL_checkinteger(L, 1));
  if (player < 0 || player > 1) return luaL_error(L, "invalid player");
  const std::string text = luaL_checkstring(L, 2);
  // GameState::PlayerIsUsingModifier: apply to copies and compare current values.
  PlayerOptions po = state->players[player].GetSong();
  SongOptions so = state->song.GetSong();
  po.FromString(text);
  so.FromString(text);
  lua_pushboolean(L, po == state->players[player].GetSong() && so == state->song.GetSong());
  return 1;
}
}

void install_option_queries(lua_State* L) {
  // Native LuaXType builds both directions used by ENUM_INTERFACE.
  LuaLifeType(L);
  LuaDrainType(L);
  LuaHideLightType(L);
  LuaModTimerType(L);
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
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
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
      PlayerOptions probe = state->players[player].GetSong();
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
    const std::string modifiers = luaL_checkstring(L, 2);
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    // GameState::ApplyStageModifiers calls the native ModsGroup at Stage.
    state->players[player].FromString(ModsLevel_Stage, modifiers);
    state->song.FromString(ModsLevel_Stage, modifiers);
    return 0;
  }, 1);
  lua_setglobal(L, "_ITG_APPLY_STAGE_MODIFIERS");
  lua_pushcclosure(L, using_modifier, 1);
  lua_setglobal(L, "_ITG_USING_MODIFIER");
}
