for i = 1, 2 do
    local judgment = SCREENMAN:GetTopScreen():GetChild("PlayerP" .. i):GetChild("Judgment")
    local sprite = judgment:GetChild("JudgmentWithOffsets")
    local texture = assert(sprite:GetTexture(), "the theme's initial judgment must be loaded")
    assert(texture:GetPath():find("Normal 2x6.png", 1, true))
    assert(sprite:GetWidth() == 64 and sprite:GetHeight() == 64)
    assert(not sprite:GetVisible())
end
return Def.Actor{}
