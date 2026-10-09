local piece = GAMESTATE:GetCurrentSong():GetSongDir() .. "../../../fixtures/actors/model-triangle.txt"
local function model(name, x, init, on)
  return Def.Model {
    Name=name, Meshes=piece, Materials=piece, Bones=piece,
    InitCommand=function(self)
      self:xy(x,200):diffuse(0.5,0.75,1,0.6):glow(1,0,0,0.25)
      init(self)
    end,
    OnCommand=on,
  }
end
return Def.ActorFrame {
  Name="Root", FOV=0,
  InitCommand=function(self)
    self:baserotationx(10):baserotationy(-15):baserotationz(20)
      :rotationx(3):rotationy(5):rotationz(7)
  end,
  model("Angles",100,function(self)
    self:baserotationx(23):baserotationy(-17):baserotationz(41)
      :rotationx(11):rotationy(13):rotationz(-19)
  end,function(self)
    self:linear(1):rotationx(5):rotationy(-7):rotationz(31)
  end),
  model("Immediate",200,function(self) self:rotationz(10) end,
    function(self) self:sleep(0.5):baserotationz(90):linear(0.5):rotationz(30) end),
  model("Replaced",300,function(self) self:baserotationz(180):baserotationz(270) end),
}
