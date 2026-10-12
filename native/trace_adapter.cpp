#include "global.h"
#include "LuaManager.h"

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
  lua_pushcfunction(L, trace_wrap);
  lua_setglobal(L, "ITGMANIA_HARNESS_WRAP");
}
REGISTER_WITH_LUA_FUNCTION(register_trace_adapter)
