#include "global.h"
#include "LuaManager.h"
#include "Actor.h"
#include "XmlFile.h"
#ifdef _WIN32
#include <cstdlib>
#include "archutils/Win32/GraphicsWindow.h"
static int trace_headless_window_visible(lua_State* L) {
  lua_pushboolean(L, GraphicsWindow::GetHwnd() && IsWindowVisible(GraphicsWindow::GetHwnd()));
  return 1;
}
#endif

// ActorUtil has set the real parent, but has not loaded children or commands.
// XNode's existing text value retains the original definition table; this
// observes that table without adding fields or callbacks to authored Lua.
void itgmania_harness_actor_created(Actor* actor, const XNode* node) {
  Lua* L = LUA->Get();
  const int top = lua_gettop(L);
  lua_getglobal(L, "ITGMANIA_HARNESS_ACTOR_CREATED");
  if (lua_isfunction(L, -1)) {
    actor->PushSelf(L);
    node->PushAttrValue(L, XNode::TEXT_ATTRIBUTE);
    if (actor->GetParent()) actor->GetParent()->PushSelf(L);
    else lua_pushnil(L);
    // MessageManager orders IMessageSubscriber*, including any base offset.
    lua_pushfstring(L, "%p", static_cast<IMessageSubscriber*>(actor));
    if (lua_pcall(L, 4, 0, 0) != 0) {
      // Reporting a script error would itself broadcast a message. Retain the
      // observer failure separately and let the capture validator reject it.
      lua_setglobal(L, "ITGMANIA_HARNESS_ACTOR_CREATION_ERROR");
    }
  }
  lua_settop(L, top);
  LUA->Release(L);
}

// A C entry point keeps Lua 5.1 from replacing the authored tail-call frame
// with a Lua observer closure. The observer runs in the same VM and receives
// every argument and return value, including nil entries and Lua errors.
static int trace_delegate(lua_State* L) {
  const int args = lua_gettop(L);
  lua_pushvalue(L, lua_upvalueindex(1));
  lua_insert(L, 1);
  lua_call(L, args, LUA_MULTRET);
  return lua_gettop(L);
}

static int trace_wrap(lua_State* L) {
  luaL_checktype(L, 1, LUA_TFUNCTION);
  lua_pushvalue(L, 1);
  lua_pushcclosure(L, trace_delegate, 1);
  return 1;
}

static void register_trace_adapter(Lua* L) {
#ifdef _WIN32
  lua_pushboolean(L, true);
  lua_setglobal(L, "ITGMANIA_HARNESS_HEADLESS_GRAPHICS_AVAILABLE");
  lua_pushboolean(L, std::getenv("ITGMANIA_HARNESS_HEADLESS_GRAPHICS") != nullptr);
  lua_setglobal(L, "ITGMANIA_HARNESS_HEADLESS_GRAPHICS");
  lua_pushcfunction(L, trace_headless_window_visible);
  lua_setglobal(L, "ITGMANIA_HARNESS_WINDOW_VISIBLE");
#endif
  lua_pushcfunction(L, trace_wrap);
  lua_setglobal(L, "ITGMANIA_HARNESS_WRAP");
  lua_pushboolean(L, true);
  lua_setglobal(L, "ITGMANIA_HARNESS_ACTOR_CREATION_AVAILABLE");
}
REGISTER_WITH_LUA_FUNCTION(register_trace_adapter)
