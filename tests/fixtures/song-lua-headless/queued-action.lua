return Def.ActorFrame {
	Name = "root",
	Def.Quad {
		Name = "flower",
		InitCommand = function(self)
			self:visible(false)
		end,
	},
	InitCommand = function(self)
		curaction = 1
		mod_actions = {
			{ 0.05, function() self:GetChild("flower"):visible(true) end },
		}
	end,
	OnCommand = function(self)
		self:sleep(0.1):queuecommand("Update")
	end,
	UpdateCommand = function(self)
		while curaction <= #mod_actions and GAMESTATE:GetSongBeat() >= mod_actions[curaction][1] do
			mod_actions[curaction][2]()
			curaction = curaction + 1
		end
		self:sleep(0.1):queuecommand("Update")
	end,
}
