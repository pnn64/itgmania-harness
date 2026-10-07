return Def.ActorFrame {
	Name = "shaking-parent",
	InitCommand = function(self)
		self:xy(100, 80):vibrate():effectmagnitude(20, 12, 4)
	end,
	Def.Quad {
		Name = "child",
		InitCommand = function(self)
			self:xy(10, 5):zoomto(40, 20)
		end,
	},
}
