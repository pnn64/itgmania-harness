local prefs = PREFSMAN
local original = prefs:GetPreference("TimingWindowAdd")
assert(original == 0)
assert(prefs:SetPreference("TimingWindowAdd", 0.005) == prefs)
assert(prefs:GetPreference("timingwindowadd") == 0.004999999888241291)
prefs:SetPreference("TIMINGWINDOWADD", "0.125")
assert(prefs:GetPreference("TimingWindowAdd") == 0.125)
prefs:SetPreference("TimingWindowAdd", false)
assert(prefs:GetPreference("TimingWindowAdd") == 0)
prefs:SetPreference("TimingWindowAdd", original)
return Def.ActorFrame {}
