return Def.ActorFrame {
	Name = "root",
	ChildCommand = function(self)
		self:x(1)
	end,
	Def.ActorFrame {
		Name = "branch",
		ChildCommand = function(self)
			self:x(2)
		end,
		Def.Quad {
			Name = "leaf",
			ChildCommand = function(self)
				self:x(3)
			end,
		},
	},
	OnCommand = function(self)
		self:playcommand("Child")
	end,
}
