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
#include "GameManager.h"
#include "PlayerStageStats.h"
#include "PlayerState.h"
#include "ScoreKeeperNormal.h"
#include "NoteDataUtil.h"
#include "Song.h"
#include "Steps.h"
#include "Style.h"
#include "TimingSegments.h"
#include <set>
#include "ModsGroup.cpp"
#include "PlayerNumber.cpp"
#include "runtime_stubs.h"

namespace {
struct OptionClock {
  int64_t previous = harness_native_time();
  OptionClock() { harness_native_time(0); }
  ~OptionClock() { harness_native_time(previous); }
};

struct OptionState {
  OptionClock clock;
  PlayerState players[2];
  PlayerStageStats stats[2];
  std::unique_ptr<ScoreKeeperNormal> scores[2];
  NoteData notes[2];
  std::set<int> judged_rows[2];
  std::set<std::pair<int, int>> judged_holds[2];
  Song* song = nullptr;
  std::unique_ptr<Song> control_song;
  bool scores_loaded = false;
  const Game* saved_game = GAMESTATE->m_pCurGame;
  const Style* saved_style[2] = {
      saved_game ? GAMESTATE->GetCurrentStyle(PLAYER_1) : nullptr,
      saved_game ? GAMESTATE->GetCurrentStyle(PLAYER_2) : nullptr};
  PlayMode saved_play = GAMESTATE->m_PlayMode;
  Song* saved_current_song = GAMESTATE->m_pCurSong;
  Steps* saved_steps[2] = {GAMESTATE->m_pCurSteps[0], GAMESTATE->m_pCurSteps[1]};
  PlayerState* saved_players[2] = {GAMESTATE->m_pPlayerState[0], GAMESTATE->m_pPlayerState[1]};
  bool saved_joined[2] = {GAMESTATE->m_bSideIsJoined[0], GAMESTATE->m_bSideIsJoined[1]};
  TimingData* saved_timing = GAMESTATE->GetProcessedTimingData();
  ModsGroup<SongOptions> saved_song = GAMESTATE->m_SongOptions;
  int64_t current_time = 0;
  bool saved_rate_pref = PREFSMAN->m_bRateModsAffectTweens;
  AllowW1 saved_allow_w1 = PREFSMAN->m_AllowW1;
  bool saved_merciful = PREFSMAN->m_bMercifulBeginner;
  NoteSkinManager* saved_skin = NOTESKIN;
  std::unique_ptr<NoteSkinManager> fallback_skin;
  OptionState() {
    players[0].SetPlayerNumber(PLAYER_1);
    players[1].SetPlayerNumber(PLAYER_2);
    stats[0].Init(PLAYER_1);
    stats[1].Init(PLAYER_2);
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
    for (auto& score : scores) score.reset();
    if (scores_loaded) {
      GAMESTATE->m_pCurGame.Set(saved_game ? saved_game : harness_score_game());
      for (int player = 0; player < 2; ++player) {
        GAMESTATE->SetCurrentStyle(saved_style[player], static_cast<PlayerNumber>(player));
        GAMESTATE->m_pCurSteps[player].Set(saved_steps[player]);
        GAMESTATE->m_pPlayerState[player] = saved_players[player];
        GAMESTATE->m_bSideIsJoined[player] = saved_joined[player];
      }
      GAMESTATE->m_pCurGame.Set(saved_game);
      GAMESTATE->m_PlayMode.Set(saved_play);
      GAMESTATE->m_pCurSong.Set(saved_current_song);
      GAMESTATE->SetProcessedTimingData(saved_timing);
    }
    GAMESTATE->m_SongOptions = saved_song;
    PREFSMAN->m_bRateModsAffectTweens.Set(saved_rate_pref);
    PREFSMAN->m_AllowW1.Set(saved_allow_w1);
    PREFSMAN->m_bMercifulBeginner.Set(saved_merciful);
    if (fallback_skin) NOTESKIN = saved_skin;
  }
  void LoadScore(int player, Steps* steps) {
    scores_loaded = true;
    GAMESTATE->m_pCurGame.Set(harness_score_game());
    harness_prepare_scoring();
    GAMESTATE->m_PlayMode.Set(PLAY_MODE_REGULAR);
    GAMESTATE->m_pCurSong.Set(song);
    GAMESTATE->m_pCurSteps[player].Set(steps);
    GAMESTATE->m_pPlayerState[player] = &players[player];
    // Canonical captures enable two sides for four-column dance and one side
    // for double. Select the native compatible style (versus for two singles),
    // rather than taking the first same-type style in the game's array.
    GAMESTATE->m_bSideIsJoined[0] = true;
    GAMESTATE->m_bSideIsJoined[1] = steps->m_StepsType != StepsType_dance_double;
    const Style* style = GAMEMAN->GetFirstCompatibleStyle(
        GAMESTATE->m_pCurGame, GAMESTATE->GetNumSidesJoined(), steps->m_StepsType);
    if (!style) throw std::runtime_error("native gameplay style is unavailable for the joined sides");
    GAMESTATE->SetCurrentStyle(style, static_cast<PlayerNumber>(player));
    scores[player].reset();
    stats[player] = PlayerStageStats();
    stats[player].Init(static_cast<PlayerNumber>(player));
    // ScreenGameplay sets the queue/joined state before ScoreKeeper::Load,
    // then increments StepsPlayed while loading the first song.
    stats[player].m_vpPossibleSteps = {steps};
    stats[player].m_bJoined = true;
    stats[player].m_pStyle = style;
    judged_rows[player].clear();
    judged_holds[player].clear();
    scores[player] = std::make_unique<ScoreKeeperNormal>(&players[player], &stats[player]);
    scores[player]->Load({song}, {steps}, {AttackArray{}});
    NoteData original;
    steps->GetNoteData(original);
    style->GetTransformedNoteDataForStyle(static_cast<PlayerNumber>(player), original, notes[player]);
    NoteDataUtil::TransformNoteData(notes[player], *steps->GetTimingData(),
                                   players[player].m_PlayerOptions.GetStage(), steps->m_StepsType);
    ++stats[player].m_iStepsPlayed;
    scores[player]->OnNextSong(0, steps, &notes[player]);
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
    LuaHelpers::Push(L, state->players[player].m_PlayerOptions.Get(level).GetString());
    return 1;
  }
  if (method == "GetMods") {
    std::vector<std::string> parts;
    state->players[player].m_PlayerOptions.Get(level).GetMods(parts);
    LuaHelpers::CreateTableFromArray(parts, L);
    return 1;
  }
  if (method == "SetPlayerOptions") {
    // LunaPlayerState::SetPlayerOptions assigns freshly parsed options; it
    // does not add modifiers to the previous PlayerOptions instance.
    PlayerOptions options;
    options.FromString(luaL_checkstring(L, 1));
    state->players[player].m_PlayerOptions.Assign(level, options);
    return 0;
  }
  // OPTIONAL_RETURN_SELF tests original_top. Index zero is not a valid Lua
  // argument; use the equivalent explicit-nil native getter to avoid reading
  // a stale slot outside the argument stack after a successful skin setter.
  if (method == "NoteSkin" && lua_gettop(L) == 0) lua_pushnil(L);
  return call_option(&state->players[player].m_PlayerOptions.Get(level), L, method, player_methods);
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
  PlayerOptions po = state->players[player].m_PlayerOptions.GetCurrent();
  SongOptions so = GAMESTATE->m_SongOptions.GetCurrent();
  po.FromString(text);
  so.FromString(text);
  lua_pushboolean(L, po == state->players[player].m_PlayerOptions.GetCurrent() && so == GAMESTATE->m_SongOptions.GetCurrent());
  return 1;
}
}

void install_option_queries(lua_State* L, bool scoring_control) {
  if (scoring_control) install_score_queries(L);
  // Native LuaXType builds both directions used by ENUM_INTERFACE.
  LuaLifeType(L);
  LuaPlayerNumber(L);
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
  lua_setfield(L, LUA_REGISTRYINDEX, "ITG_SCORE_STATE");
  GAMESTATE->PushSelf(L);
  lua_setglobal(L, "_ITG_NATIVE_GAMESTATE");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int index = luaL_checkint(L, 1);
    if (!state->song || state->song->GetAllSteps().empty()) { lua_pushnil(L); return 1; }
    const auto& steps = state->song->GetAllSteps();
    if (index < 1 || index > static_cast<int>(steps.size())) return luaL_error(L, "invalid native Steps index");
    steps[index - 1]->PushSelf(L);
    return 1;
  }, 1);
  lua_setglobal(L, "_ITG_NATIVE_STEPS");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = Enum::Check<PlayerNumber>(L, 1);
    const int index = luaL_checkint(L, 2);
    if (!state->song || state->song->GetAllSteps().empty()) {
      lua_pushboolean(L, false); return 1;
    }
    const auto& steps = state->song->GetAllSteps();
    if (index < 1 || index > static_cast<int>(steps.size())) return luaL_error(L, "invalid native score chart");
    try {
      state->LoadScore(player, steps[index - 1]);
    } catch (const std::exception& error) {
      return luaL_error(L, "%s", error.what());
    }
    lua_pushboolean(L, true); return 1;
  }, 1);
  lua_setglobal(L, "_ITG_STAGE_SCORE_LOAD");
  if (scoring_control) {
    lua_pushvalue(L, -1);
    lua_pushcclosure(L, [](lua_State* L) -> int {
      auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
      const int player = Enum::Check<PlayerNumber>(L, 1);
      const std::string text = luaL_checkstring(L, 2);
      const Difficulty difficulty = Enum::Check<Difficulty>(L, 3);
      const float seconds = lua_gettop(L) >= 4 ? FArg(4) : 2.0f;
      if (!std::isfinite(seconds) || seconds <= 0) return luaL_error(L, "invalid score chart length");
      if (!state->control_song) {
        state->control_song = std::make_unique<Song>();
        state->control_song->m_SongTiming.AddSegment(BPMSegment(0, 120.0f));
        state->control_song->m_SongTiming.TidyUpData(false);
      }
      state->song = state->control_song.get();
      state->song->m_fMusicLengthSeconds = seconds;
      auto* steps = state->song->CreateSteps();
      steps->m_StepsType = StepsType_dance_single;
      steps->m_StepsTypeStr = "dance-single";
      steps->SetDifficulty(difficulty);
      steps->SetSMNoteData(text);
      state->song->AddSteps(steps);
      try {
        state->LoadScore(player, steps);
      } catch (const std::exception& error) {
        return luaL_error(L, "%s", error.what());
      }
      return 0;
    }, 1);
    lua_setglobal(L, "_ITG_SCORE_CHART");
    lua_pushvalue(L, -1);
    lua_pushcclosure(L, [](lua_State* L) -> int {
      auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
      const int player = Enum::Check<PlayerNumber>(L, 1);
      const int row = luaL_checkint(L, 2);
      const TapNoteScore score = Enum::Check<TapNoteScore>(L, 3);
      if (!state->scores[player] || score < TNS_Miss || score > TNS_W1 ||
          state->judged_rows[player].count(row) ||
          !GAMESTATE->m_pCurSteps[player]->GetTimingData()->IsJudgableAtRow(row))
        return luaL_error(L, "invalid or repeated score row");
      auto& notes = state->notes[player];
      int count = 0;
      for (int track = 0; track < notes.GetNumTracks(); ++track) {
        auto note = notes.GetTapNote(track, row);
        if (note.type == TapNoteType_Tap || note.type == TapNoteType_HoldHead || note.type == TapNoteType_Lift) {
          note.result.tns = score;
          note.result.fTapNoteOffset = 0.0f;
          notes.SetTapNote(track, row, note);
          ++count;
        }
      }
      if (!count) return luaL_error(L, "native score row has no taps");
      state->judged_rows[player].insert(row);
      try {
        for (int track = 0; track < notes.GetNumTracks(); ++track) {
          const auto& note = notes.GetTapNote(track, row);
          if (note.type == TapNoteType_Tap || note.type == TapNoteType_HoldHead || note.type == TapNoteType_Lift)
            state->scores[player]->HandleTapScore(note);
        }
        state->scores[player]->HandleTapRowScore(notes, row);
      } catch (const std::exception& error) {
        return luaL_error(L, "%s", error.what());
      }
      return 0;
    }, 1);
    lua_setglobal(L, "_ITG_SCORE_ROW");
    lua_pushvalue(L, -1);
    lua_pushcclosure(L, [](lua_State* L) -> int {
      auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
      const int player = Enum::Check<PlayerNumber>(L, 1);
      const int track = luaL_checkint(L, 2);
      const int row = luaL_checkint(L, 3);
      const HoldNoteScore score = Enum::Check<HoldNoteScore>(L, 4);
      auto& notes = state->notes[player];
      if (!state->scores[player] || track < 0 || track >= notes.GetNumTracks() ||
          score == HNS_None || state->judged_holds[player].count({track, row}))
        return luaL_error(L, "invalid or repeated hold judgment");
      auto note = notes.GetTapNote(track, row);
      if (note.type != TapNoteType_HoldHead) return luaL_error(L, "native score note is not a hold");
      note.HoldResult.hns = score;
      notes.SetTapNote(track, row, note);
      state->judged_holds[player].insert({track, row});
      state->scores[player]->HandleHoldScore(note);
      return 0;
    }, 1);
    lua_setglobal(L, "_ITG_SCORE_HOLD");
  }
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    state->stats[player].SetLifeRecordAt(static_cast<float>(luaL_checknumber(L, 2)),
                                        static_cast<float>(luaL_checknumber(L, 3)));
    return 0;
  }, 1);
  lua_setglobal(L, "_ITG_STAGE_LIFE_RECORD");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    state->stats[Enum::Check<PlayerNumber>(L, 1)].PushSelf(L);
    return 1;
  }, 1);
  lua_setglobal(L, "_ITG_STAGE_STATS");
  lua_pushvalue(L, -1);
  lua_pushcclosure(L, [](lua_State* L) -> int {
    auto* state = static_cast<OptionState*>(lua_touserdata(L, lua_upvalueindex(1)));
    // Player::Load seeds SetLifeRecordAt before Foreground::LoadFromSong.
    // Simply Love and _fallback both set LifeMeterBar.InitialValue to 0.5.
    const int player = static_cast<int>(luaL_checkinteger(L, 1));
    if (player < 0 || player > 1) return luaL_error(L, "invalid player");
    const auto drain = state->players[player].m_PlayerOptions.GetStage().m_DrainType;
    if (drain != DrainType_Normal && drain != DrainType_NoRecover &&
        drain != DrainType_SuddenDeath) return luaL_error(L, "invalid drain type");
    state->stats[player].SetLifeRecordAt(drain == DrainType_Normal ? 0.5f : 1.0f, 0.0f);
    return 0;
  }, 1);
  lua_setglobal(L, "_ITG_STAGE_LIFE_SEED");
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
    state->players[player].m_PlayerOptions.Assign(ModsLevel_Preferred, state->players[player].m_PlayerOptions.GetSong());
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
      PlayerOptions probe = state->players[player].m_PlayerOptions.Get(level);
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
      const PlayerOptions& current = state->players[player].m_PlayerOptions.Get(level);
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
    state->players[player].m_PlayerOptions.FromString(ModsLevel_Stage, modifiers);
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
      "_ITG_USING_MODIFIER", "_ITG_STAGE_LIFE_RECORD",
      "_ITG_STAGE_LIFE_SEED", "_ITG_STAGE_STATS", "_ITG_STAGE_SCORE_LOAD",
      "_ITG_SCORE_CHART", "_ITG_SCORE_ROW", "_ITG_SCORE_HOLD",
      "_ITG_NATIVE_GAMESTATE", "_ITG_NATIVE_STEPS"}) {
    lua_pushnil(L); lua_setglobal(L, name);
  }
  lua_pushnil(L); lua_setfield(L, LUA_REGISTRYINDEX, "ITG_SCORE_STATE");
  lua_gc(L, LUA_GCCOLLECT, 0);
  clear_score_queries(L);
}

void bind_score_song(lua_State* L, Song* song) {
  lua_getfield(L, LUA_REGISTRYINDEX, "ITG_SCORE_STATE");
  auto* state = static_cast<OptionState*>(lua_touserdata(L, -1));
  if (!state) throw std::runtime_error("native score state not installed");
  state->song = song;
  lua_pop(L, 1);
}
