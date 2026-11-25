#pragma once
#include <string>

class Actor;

void itgmania_harness_foreground_event(const char* phase, const Actor* actor,
    float start_beat, float stop_beat, const std::string& path = {});
