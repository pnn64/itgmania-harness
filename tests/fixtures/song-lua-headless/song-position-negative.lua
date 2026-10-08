return Def.ActorFrame {
    OnCommand=function(self)
        self:SetUpdateFunction(function()
            local native=_ITG_NATIVE_SONG_POSITION(GetTimeSinceStart())
            local expected=native:GetMusicSeconds()
            local position=GAMESTATE:GetSongPosition()
            assert(position:GetMusicSeconds()==expected,
                string.format("public music seconds %s != native SongPosition %s", position:GetMusicSeconds(), expected))
            assert(position:GetMusicSecondsVisible()==native:GetMusicSecondsVisible())
            assert(GAMESTATE:GetCurMusicSeconds()==expected)
            for _, pn in ipairs({PLAYER_1, PLAYER_2}) do
                assert(GAMESTATE:GetPlayerState(pn):GetSongPosition():GetMusicSeconds()==expected)
            end
        end)
    end,
}
