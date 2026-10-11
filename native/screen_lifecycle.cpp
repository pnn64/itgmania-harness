// Execute unchanged ScreenManager dispatch methods around the headless scene
// factory. Rendering, audio-device synchronization and resource eviction stay
// outside this adapter; scene lifetime and command order execute native source.
#include "runtime_stubs.h"
#include "global.h"
#include "LuaManager.h"
#include "MessageManager.h"
#include "RageLog.h"
#include "RageUtil.h"
#include "ScreenMessage.h"
#include "PlayerNumber.h"
#include <algorithm>
#include <cstring>
#include <memory>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

struct SongLuaScreens {
  lua_State* state;
  int callback = LUA_NOREF;
  int generation = 0;
  void Call(const char* operation, int scene = 0, float delta = 0,
            const std::string& text = "") {
    if (callback == LUA_NOREF) return;
    const int top = lua_gettop(state);
    lua_rawgeti(state, LUA_REGISTRYINDEX, callback);
    lua_pushstring(state, operation);
    lua_pushinteger(state, scene);
    lua_pushnumber(state, delta);
    lua_pushlstring(state, text.data(), text.size());
    if (lua_pcall(state, 4, 0, 0)) {
      const char* value = lua_tostring(state, -1);
      std::string error = value ? value : "native scene callback failed";
      lua_settop(state, top);
      throw std::runtime_error(error);
    }
    lua_settop(state, top);
  }
};

namespace song_screen {
SongLuaScreens* context = nullptr;
class Actor {
 public:
  virtual ~Actor() = default;
  virtual void Update(float) {}
};
class Screen : public Actor {
 public:
  explicit Screen(std::string name, int generation) : name_(std::move(name)), scene(generation) {}
  ~Screen() override = default;
  const std::string& GetName() const { return name_; }
  bool IsFirstUpdate() const { return first_; }
  void Update(float delta) override {
    context->Call("update", scene, delta);
    first_ = false;
  }
  void BeginScreen() { context->Call("begin", scene); }
  void EndScreen() { context->Call("end", scene); }
  void HandleScreenMessage(ScreenMessage message) {
    context->Call("screen-message", scene, 0, ScreenMessageHelpers::ScreenMessageToString(message));
  }
  int scene;
 private:
  std::string name_;
  bool first_ = true;
};
class ScreenManager {
 public:
  ScreenMessage m_PopTopScreen = SM_Invalid;
  std::string m_sDelayedScreen;
  bool m_bZeroNextUpdate = true;
  void SetNewScreen(const std::string& name);
  void Update(float delta);
  void LoadDelayedScreen();
  ScreenMessage PopTopScreenInternal(bool lose_focus = true);
  Screen* GetTopScreen();
  bool IsScreenNameValid(const std::string& name) const { return name == "ScreenGameplay"; }
  bool ActivatePreparedScreenAndBackground(const std::string& name);
  void PrepareScreen(const std::string& name);
  void SendMessageToTopScreen(ScreenMessage message) {
    if (auto* screen = GetTopScreen()) screen->HandleScreenMessage(message);
  }
  void PostMessageToTopScreen(ScreenMessage, float) {}
  void RefreshCreditsMessages() { context->Call("broadcast", 0, 0, "RefreshCreditText"); }
  void ZeroNextUpdate() { m_bZeroNextUpdate = true; }
};
struct LoadedScreen;
std::vector<LoadedScreen> g_ScreenStack, g_vPreparedScreens;
std::vector<Screen*> g_OverlayScreens;
std::set<std::string> g_setGroupedScreens, g_setPersistantScreens;
std::string m_sPreviousTopScreen;
Actor shared_background;
Actor* g_pSharedBGA = &shared_background;
ScreenManager manager;
ScreenManager* SCREENMAN = &manager;
bool g_bDelayedScreenLoad = false;
struct SoundBoundary { void Flush() {} } sound_boundary;
SoundBoundary* SOUND = &sound_boundary;
struct MessageBoundary {
  void Broadcast(MessageID message) {
    context->Call("broadcast", 0, 0, MessageIDToString(message));
  }
} message_boundary;
MessageBoundary* MESSAGEMAN = &message_boundary;
namespace LuaHelpers {
void ReportScriptError(const std::string& message, const std::string& = "") {
  context->Call("error", 0, 0, message);
}
}
void BeforeDeleteScreen() { manager.ZeroNextUpdate(); }
void AfterDeleteScreen() {}
void GrabPreparedActors(std::vector<Actor*>& actors);
void DeletePreparedScreens();

#include "screen_lifecycle.inc"

Screen* ScreenManager::GetTopScreen() {
  return g_ScreenStack.empty() ? nullptr : g_ScreenStack.back().m_pScreen;
}
void GrabPreparedActors(std::vector<Actor*>& actors) {
  for (const auto& loaded : g_vPreparedScreens)
    if (loaded.m_bDeleteWhenDone) actors.push_back(loaded.m_pScreen);
  g_vPreparedScreens.clear();
  g_setGroupedScreens.clear();
  g_setPersistantScreens.clear();
}
void DeletePreparedScreens() {
  std::vector<Actor*> actors;
  GrabPreparedActors(actors);
  BeforeDeleteScreen();
  for (Actor* actor : actors) delete actor;
  AfterDeleteScreen();
}
bool ScreenManager::ActivatePreparedScreenAndBackground(const std::string& name) {
  if (GetTopScreen() && GetTopScreen()->GetName() == name) return true;
  LoadedScreen loaded;
  if (!GetPreppedScreen(name, loaded)) return false;
  PushLoadedScreen(loaded);
  return true;
}
void ScreenManager::PrepareScreen(const std::string& name) {
  if (ScreenIsPrepped(name)) return;
  const int scene = ++context->generation;
  context->Call("load", scene, 0, name);
  LoadedScreen loaded;
  loaded.m_pScreen = new Screen(name, scene);
  g_vPreparedScreens.push_back(loaded);
}
void Reset() {
  // Session teardown does not execute Lua after the host has serialized its
  // trace. Native pop/end calls are executed during live replacement instead.
  for (const auto& loaded : g_ScreenStack) delete loaded.m_pScreen;
  g_ScreenStack.clear();
  DeletePreparedScreens();
  manager = ScreenManager{};
  m_sPreviousTopScreen.clear();
}
}

namespace {
SongLuaScreens& Owner(lua_State* L) {
  return *static_cast<SongLuaScreens*>(lua_touserdata(L, lua_upvalueindex(1)));
}
int ScreenCall(lua_State* L) {
  // Validate Lua arguments before owning C++ values: luaL_* may longjmp.
  const char* operation = luaL_checkstring(L, 1);
  const bool initialize = std::strcmp(operation, "init") == 0;
  const bool request = std::strcmp(operation, "request") == 0;
  const bool update = std::strcmp(operation, "update") == 0;
  if (initialize) luaL_checktype(L, 2, LUA_TFUNCTION);
  const char* name = request ? luaL_checkstring(L, 2) : nullptr;
  if (name && !*name) return luaL_error(L, "native screen name must not be empty");
  const float delta = update ? static_cast<float>(luaL_checknumber(L, 2)) : 0;
  bool failed = false;
  {
    try {
      auto& owner = Owner(L);
      if (initialize) {
        if (owner.callback != LUA_NOREF) throw std::runtime_error("native screen session already initialized");
        lua_pushvalue(L, 2);
        owner.callback = luaL_ref(L, LUA_REGISTRYINDEX);
        song_screen::LoadedScreen loaded;
        loaded.m_pScreen = new song_screen::Screen("ScreenGameplay", 0);
        song_screen::g_ScreenStack.push_back(loaded);
      } else if (request) {
        song_screen::manager.SetNewScreen(name);
      } else if (update) {
        song_screen::manager.Update(delta);
      } else if (std::strcmp(operation, "top") == 0) {
        auto* screen = song_screen::manager.GetTopScreen();
        if (screen) lua_pushinteger(L, screen->scene); else lua_pushnil(L);
        return 1;
      } else if (std::strcmp(operation, "first") == 0) {
        auto* screen = song_screen::manager.GetTopScreen();
        lua_pushboolean(L, screen && screen->IsFirstUpdate());
        return 1;
      } else {
        throw std::runtime_error("unknown native screen operation");
      }
    } catch (const std::exception& error) {
      lua_pushstring(L, error.what());
      failed = true;
    }
  }
  return failed ? lua_error(L) : 0;
}
}

SongLuaScreens* install_song_screens(lua_State* state) {
  auto screens = std::make_unique<SongLuaScreens>();
  screens->state = state;
  song_screen::context = screens.get();
  song_screen::Reset();
  lua_pushlightuserdata(state, screens.get());
  lua_pushcclosure(state, ScreenCall, 1);
  lua_setglobal(state, "_ITG_SCREEN_DISPATCH");
  return screens.release();
}
void destroy_song_screens(SongLuaScreens* screens) {
  if (!screens) return;
  song_screen::Reset();
  if (screens->callback != LUA_NOREF) luaL_unref(screens->state, LUA_REGISTRYINDEX, screens->callback);
  song_screen::context = nullptr;
  delete screens;
}
