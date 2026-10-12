# Include after the top-level project() through CMAKE_PROJECT_INCLUDE.
function(itgmania_harness_trace_adapter)
  target_sources(ITGmania PRIVATE "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/trace_adapter.cpp")
  # Retain the stock executable for historical captures and independent checks.
  set_target_properties(ITGmania PROPERTIES OUTPUT_NAME_RELEASE ITGmania-harness-trace)
endfunction()
if(CMAKE_CURRENT_SOURCE_DIR STREQUAL CMAKE_SOURCE_DIR)
  cmake_language(DEFER CALL itgmania_harness_trace_adapter)
endif()
