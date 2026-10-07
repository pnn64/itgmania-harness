return Def.ActorFrame {
	Def.Quad {
		Name = "hidden",
		InitCommand = function(self)
			self:zoomto(40, 20):visible(0)
		end,
	},
	Def.Quad {
		Name = "shown",
		InitCommand = function(self)
			self:zoomto(40, 20):visible(1)
		end,
	},
}
