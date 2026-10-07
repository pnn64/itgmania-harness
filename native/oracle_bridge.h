#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ItgOracleBuffer {
  uint8_t* data;
  size_t len;
} ItgOracleBuffer;

ItgOracleBuffer itg_oracle_load_charts(const uint8_t* path, size_t path_len,
    const uint8_t* theme, size_t theme_len, const uint8_t* host, size_t host_len);
ItgOracleBuffer itg_oracle_load_font(const uint8_t* path, size_t path_len,
                                     const uint8_t* text, size_t text_len,
                                     uint8_t mapped_only);
ItgOracleBuffer itg_oracle_load_noteskin(
    const uint8_t* root, size_t root_len, const uint8_t* game,
    size_t game_len, const uint8_t* skin, size_t skin_len,
    const uint8_t* request, size_t request_len);
ItgOracleBuffer itg_oracle_load_song_lua(const uint8_t* path,
                                         size_t path_len);
ItgOracleBuffer itg_oracle_eval_song_lua(
    const uint8_t* request, size_t request_len, const uint8_t* host,
    size_t host_len);
ItgOracleBuffer itg_oracle_eval_actor_fixture(const uint8_t* request,
                                              size_t request_len);
void itg_oracle_free(uint8_t* data);

#ifdef __cplusplus
}
#endif
