#pragma once

#include <string>
#include <vector>
#include <mutex>

// ITGmania's singleton managers and diagnostics are shared by every oracle.
// Hold this for the entire native call, including manager teardown.
std::mutex& harness_native_mutex();

struct lua_State;
void harness_register_lua_globals(lua_State* state);

void harness_configure_font_paths(const std::string& theme_fonts,
                                  const std::string& fallback_fonts);
void harness_clear_diagnostics();
std::vector<std::string> harness_take_diagnostics();
