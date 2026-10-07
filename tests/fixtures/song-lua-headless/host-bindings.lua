local child = LoadActor("host-child.lua", false, 5, nil, "ok")
local root_ready, child_ready = false, false
child.InitCommand = function(self)
    child_ready = true
    assert(type(self) == "userdata")
    rec_print_table(self)
end
return Def.ActorFrame {
    Name = "HostBindings",
    InitCommand = function(self)
        assert(child_ready)
        assert(self:GetChildAt(0) == self:GetChild("Child"))
        root_ready = true
    end,
    BeginCommand = function(self)
        assert(root_ready)
        local options = GAMESTATE:GetPlayerState(PLAYER_1):GetPlayerOptions("ModsLevel_Song")
        options:Dark(0.3, 7)
        local dark, speed = options:Dark()
        assert(math.abs(dark - 0.3) < 0.00001 and speed == 7)
        GAMESTATE:ApplyStageModifiers(PLAYER_1, "3x, reverse")
        assert(options:ScrollSpeed() == 3 and options:Reverse() == 1)
        assert(GAMESTATE:IsEventMode())
        assert(type(PREFSMAN:GetPreference("LastSeenVideoDriver")) == "string")
        assert(SCREENMAN:GetTopScreen():GetPlayerInfo(0):GetLifeMeter():GetLife() == 0.5)
        self:xy(nil, 7)
        local file = RageFileUtil.CreateRageFile()
        assert(file:Open(GAMESTATE:GetCurrentSong():GetSongDir() .. "host-data.txt", 1))
        assert(file:GetLine() == "alpha")
        assert(file:GetLine() == "beta")
        assert(file:Read() == "" and file:AtEOF())
        file:destroy()
        local output = RageFileUtil.CreateRageFile()
        assert(not output:Open(GAMESTATE:GetCurrentSong():GetSongDir() .. "forbidden.txt", 2))
        output:destroy()
        local files = FILEMAN:GetDirListing(GAMESTATE:GetCurrentSong():GetSongDir() .. "host-*.txt", false, false)
        assert(#files == 1 and files[1] == "host-data.txt")
    end,
    child,
    Def.Sound {
        Name = "Sound",
        File = "sound-clip.wav",
        OnCommand = function(self) self:get():play() end,
    },
}
