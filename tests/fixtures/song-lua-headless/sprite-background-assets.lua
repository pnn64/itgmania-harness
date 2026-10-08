return Def.Sprite{
 InitCommand=function(self)
  assert(self:LoadFromCurrentSongBackground()==self)
  assert(self:GetWidth()==64 and self:GetHeight()==32)
  self:Load(nil)
  self:visible(false)
 end,
}
