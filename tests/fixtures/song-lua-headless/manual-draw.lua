local beat = 0
local vertices = {
  {{0, 0, 0}, {1, 1, 1, 1}, {0, 0}},
  {{0, 10, 0}, {1, 1, 1, 1}, {0, 1}},
  {{10, 0, 0}, {1, 1, 1, 1}, {1, 0}},
  {{10, 10, 0}, {1, 1, 1, 1}, {1, 1}},
}
return Def.ActorFrame {
  Name = "DrawRoot",
  InitCommand = function(self) self:xy(40, 50):SetFOV(45) end,
  OnCommand = function(self)
    local screen = SCREENMAN:GetTopScreen()
    local foreground = screen:GetChild("SongForeground")
    assert(self:GetParent() == foreground)
    assert(foreground:GetChildren().DrawRoot == self)
    local children = screen:GetChildren()
    for _, name in ipairs({"LifeMeter", "LifeMeterBarP1", "SongTitle", "BPMDisplay"}) do
      assert(children[name] == nil, name .. " is not a top-level screen child")
    end
    local engine_hud = {}
    for _, name in ipairs({"LifeP1", "LifeP2", "ScoreP1", "ScoreP2", "StepsDisplayP1", "StepsDisplayP2"}) do
      assert(children[name]:GetVisible(), "hibernation preserves visibility")
      engine_hud[#engine_hud + 1] = children[name]
    end
    local target, mesh = self:GetChild("target"), self:GetChild("mesh")
    target:SetSize(640, 480):EnableFloat(true):EnableAlphaBuffer(true)
      :EnablePreserveTexture(true):Create()
    local texture = target:GetTexture()
    mesh:SetTexture(texture):SetVertices(vertices)
      :SetDrawState{Mode="DrawMode_QuadStrip", First=1, Num=-1}
    self:SetUpdateFunction(function() beat = GAMESTATE:GetSongBeat() end)
    self:SetDrawFunction(function()
      -- The texture's explicit argument overrides the actor preserve flag.
      texture:BeginRenderingTo(beat >= 1)
      mesh:xy(beat, 0):diffuse(1, 0, 0, 1):Draw()
      mesh:xy(beat + 20, 0):diffuse(0, 1, 0, 1):Draw()
      vertices[1][1][2] = beat
      mesh:SetVertices(vertices):xy(beat + 40, 0):diffuse(0, 0, 1, 1):Draw()
      texture:FinishRenderingTo()
      SCREENMAN:GetTopScreen():GetChild("PlayerP1"):Draw()
      -- Explicit Draw and visible(true) must respect the theme's hibernation.
      for _, actor in ipairs(engine_hud) do actor:visible(true):Draw() end
    end)
  end,
  Def.ActorFrameTexture {Name="target"},
  Def.ActorMultiVertex {Name="mesh"},
}
