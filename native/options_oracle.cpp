// Include the native implementations so queries use their actual Lua setters,
// FromString parsers and equality operators, without changing reference sources.
#include "PlayerOptions.cpp"
#define AddPart SongOptionsAddPart
#include "SongOptions.cpp"
#undef AddPart
#include <new>

namespace {
struct OptionState {
  PlayerOptions players[2];
  SongOptions song;
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
    return call_option(&state->song, L, method, song_methods);
  }
  if (player < 0 || player > 1) return luaL_error(L, "invalid player");
  if (method == "GetString") {
    LuaHelpers::Push(L, state->players[player].GetString());
    return 1;
  }
  if (method == "SetPlayerOptions") {
    // LunaPlayerState::SetPlayerOptions assigns freshly parsed options; it
    // does not add modifiers to the previous PlayerOptions instance.
    PlayerOptions options;
    options.FromString(luaL_checkstring(L, 1));
    state->players[player] = options;
    return 0;
  }
  return call_option(&state->players[player], L, method, player_methods);
}

int using_modifier(lua_State* L) {
  auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
  const int player = static_cast<int>(luaL_checkinteger(L, 1));
  if (player < 0 || player > 1) return luaL_error(L, "invalid player");
  const std::string text = luaL_checkstring(L, 2);
  // GameState::PlayerIsUsingModifier: apply to copies and compare current values.
  PlayerOptions po = state->players[player];
  SongOptions so = state->song;
  po.FromString(text);
  so.FromString(text);
  lua_pushboolean(L, po == state->players[player] && so == state->song);
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
  lua_pushcclosure(L, using_modifier, 1);
  lua_setglobal(L, "_ITG_USING_MODIFIER");
}
