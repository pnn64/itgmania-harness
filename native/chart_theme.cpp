// Native-backed object surface for unchanged Simply Love chart scripts.
#include "Steps.h"
#include "TimingData.h"
#include "CryptManager.h"
#include "RageFile.h"
#include "RageUtil.h"
#include "LuaManager.h"
#include "json/json.h"
#include <cmath>
#include <algorithm>
#include <filesystem>
#include <memory>
#include <stdexcept>

int LuaFunc_BinaryToHex(lua_State* L);

namespace {
Steps* chart(lua_State* L) {
  return static_cast<Steps*>(lua_touserdata(L, lua_upvalueindex(1)));
}
PlayerNumber player(lua_State* L) {
  const std::string value = luaL_optstring(L, 2, "PlayerNumber_P1");
  if (value == "PlayerNumber_P1") return PLAYER_1;
  if (value == "PlayerNumber_P2") return PLAYER_2;
  luaL_error(L, "invalid player number");
  return PLAYER_1;
}
void method(lua_State* L, Steps& steps, const char* name, lua_CFunction fn) {
  lua_pushlightuserdata(L, &steps);
  lua_pushcclosure(L, fn, 1);
  lua_setfield(L, -2, name);
}
template<class T> void array(lua_State* L, const std::vector<T>& values) {
  lua_createtable(L, static_cast<int>(values.size()), 0);
  int i = 1;
  for (T value : values) {
    lua_pushnumber(L, value);
    lua_rawseti(L, -2, i++);
  }
}
int push_tech(lua_State* L) {
  const auto& tech = chart(L)->GetTechCounts(player(L));
  lua_newtable(L);
  lua_newtable(L);
  for (int i = 0; i < NUM_TechCountsCategory; ++i) {
    const std::string key = "TechCountsCategory_" + TechCountsCategoryToString(static_cast<TechCountsCategory>(i));
    lua_pushnumber(L, tech[i]); lua_setfield(L, -2, key.c_str());
  }
  lua_pushcclosure(L, [](lua_State* L) {
    lua_getfield(L, lua_upvalueindex(1), luaL_checkstring(L, 2));
    if (lua_isnil(L, -1)) return luaL_error(L, "unknown tech category");
    return 1;
  }, 1);
  lua_setfield(L, -2, "GetValue"); return 1;
}
void bind_steps(lua_State* L, Steps& steps) {
  lua_newtable(L);
  method(L, steps, "GetFilename", [](lua_State* L) {
    lua_pushstring(L, chart(L)->GetFilename().c_str()); return 1;
  });
  method(L, steps, "GetDescription", [](lua_State* L) {
    lua_pushstring(L, chart(L)->GetDescription().c_str()); return 1;
  });
  method(L, steps, "GetDifficulty", [](lua_State* L) {
    const std::string value = "Difficulty_" + DifficultyToString(chart(L)->GetDifficulty());
    lua_pushstring(L, value.c_str()); return 1;
  });
  method(L, steps, "GetStepsType", [](lua_State* L) {
    std::string value = "StepsType_" + chart(L)->m_StepsTypeStr;
    std::replace(value.begin(), value.end(), '-', '_');
    lua_pushstring(L, value.c_str()); return 1;
  });
  method(L, steps, "GetNotesPerMeasure", [](lua_State* L) {
    array(L, chart(L)->GetNotesPerMeasure(player(L))); return 1;
  });
  method(L, steps, "GetNpsPerMeasure", [](lua_State* L) {
    array(L, chart(L)->GetNpsPerMeasure(player(L))); return 1;
  });
  method(L, steps, "GetPeakNps", [](lua_State* L) {
    lua_pushnumber(L, chart(L)->GetPeakNps(player(L))); return 1;
  });
  method(L, steps, "GetTechCounts", push_tech);
  method(L, steps, "CalculateTechCounts", [](lua_State* L) {
    chart(L)->CalculateTechCounts(); return push_tech(L);
  });
  method(L, steps, "GetDisplayBpms", [](lua_State* L) {
    DisplayBpms bpm; chart(L)->GetDisplayBpms(bpm);
    array(L, std::vector<float>{bpm.GetMin(), bpm.GetMax()}); return 1;
  });
  method(L, steps, "GetTimingData", [](lua_State* L) {
    Steps* steps = chart(L);
    lua_newtable(L);
    method(L, *steps, "GetActualBPM", [](lua_State* L) {
      float low, high; chart(L)->GetTimingData()->GetActualBPM(low, high);
      array(L, std::vector<float>{low, high}); return 1;
    });
    method(L, *steps, "GetElapsedTimeFromBeat", [](lua_State* L) {
      lua_pushnumber(L, chart(L)->GetTimingData()->GetElapsedTimeFromBeat(
          static_cast<float>(luaL_checknumber(L, 2)))); return 1;
    });
    return 1;
  });
  lua_setglobal(L, "_STEPS");
}
std::string read_file(const std::string& path) {
  RageFile file;
  if (!file.Open(path, RageFile::READ)) throw std::runtime_error("could not read " + path);
  std::string bytes;
  if (file.Read(bytes) < 0) throw std::runtime_error("could not read " + path);
  return bytes;
}
void run(lua_State* L, const std::string& bytes, const std::string& name, int results = 0) {
  if (luaL_loadbuffer(L, bytes.data(), bytes.size(), name.c_str()) ||
      lua_pcall(L, 0, results, 0)) {
    throw std::runtime_error(lua_tostring(L, -1));
  }
}
Json::Value json(lua_State* L, int index, int depth = 0) {
  if (depth > 32) throw std::runtime_error("theme output exceeds nesting limit");
  if (index < 0) index = lua_gettop(L) + index + 1;
  switch (lua_type(L, index)) {
    case LUA_TNIL: return Json::Value();
    case LUA_TBOOLEAN: return Json::Value(lua_toboolean(L, index) != 0);
    case LUA_TNUMBER: {
      const double value = lua_tonumber(L, index);
      if (!std::isfinite(value)) throw std::runtime_error("non-finite theme output");
      return Json::Value(value);
    }
    case LUA_TSTRING: {
      size_t len; const char* value = lua_tolstring(L, index, &len);
      return Json::Value(std::string(value, len));
    }
    case LUA_TTABLE: {
      bool object = false;
      lua_pushnil(L);
      while (lua_next(L, index)) {
        object |= lua_type(L, -2) == LUA_TSTRING;
        lua_pop(L, 1);
      }
      Json::Value result(object ? Json::objectValue : Json::arrayValue);
      if (object) {
        lua_pushnil(L);
        while (lua_next(L, index)) {
          if (lua_type(L, -2) != LUA_TSTRING) throw std::runtime_error("mixed theme table keys");
          result[lua_tostring(L, -2)] = json(L, -1, depth + 1);
          lua_pop(L, 1);
        }
      } else {
        for (size_t i = 1; i <= lua_objlen(L, index); ++i) {
          lua_rawgeti(L, index, static_cast<int>(i));
          result.append(json(L, -1, depth + 1)); lua_pop(L, 1);
        }
      }
      return result;
    }
    default: throw std::runtime_error("unsupported theme output type");
  }
}
}  // namespace

std::string harness_chart_theme(Steps& steps, const std::string& theme,
    const std::string& host, bool ambiguous) {
  Json::Value result(Json::objectValue);
  try {
    std::unique_ptr<lua_State, decltype(&lua_close)> state(luaL_newstate(), lua_close);
    if (!state) throw std::runtime_error("could not create theme Lua state");
    lua_State* L = state.get();
    luaL_openlibs(L);
    lua_pushcfunction(L, LuaFunc_BinaryToHex); lua_setglobal(L, "BinaryToHex");
    bind_steps(L, steps);
    lua_pushboolean(L, ambiguous); lua_setglobal(L, "_AMBIGUOUS");
    lua_pushcfunction(L, [](lua_State* L) {
      const Difficulty difficulty = OldStyleStringToDifficulty(luaL_checkstring(L, 1));
      if (difficulty == Difficulty_Invalid) { lua_pushnil(L); return 1; }
      const std::string value = "Difficulty_" + DifficultyToString(difficulty);
      lua_pushstring(L, value.c_str()); return 1;
    }); lua_setglobal(L, "OldStyleStringToDifficulty");
    lua_pushcfunction(L, [](lua_State* L) {
      size_t len; const char* value = luaL_checklstring(L, 2, &len);
      const std::string digest = CryptManager::GetSHA1ForString(std::string(value, len));
      lua_pushlstring(L, digest.data(), digest.size()); return 1;
    }); lua_setglobal(L, "_SHA1");
    // RageFile semantics, without exposing writable files to the theme.
    lua_pushcfunction(L, [](lua_State* L) {
      try {
        const std::string bytes = read_file(luaL_checkstring(L, 1));
        lua_pushlstring(L, bytes.data(), bytes.size()); return 1;
      } catch (const std::exception& error) { return luaL_error(L, "%s", error.what()); }
    }); lua_setglobal(L, "_READ_FILE");
    run(L, host, "@chart_theme.lua");
    for (const char* name : {"SL-ChartParser.lua", "SL-ChartParserHelpers.lua", "SL-BPMDisplayHelpers.lua"}) {
      const std::string relative = std::string("Scripts/") + name;
      run(L, read_file((std::filesystem::u8path(theme) / relative).u8string()), "@" + relative);
    }
    run(L, "return _CAPTURE_THEME()", "@chart_theme_capture", 1);
    result = json(L, -1);
  } catch (const std::exception& error) {
    result["status"] = "unavailable";
    result["errors"] = Json::Value(Json::arrayValue);
    result["errors"].append(error.what());
  }
  Json::StreamWriterBuilder writer;
  writer["indentation"] = "";
  return Json::writeString(writer, result);
}
