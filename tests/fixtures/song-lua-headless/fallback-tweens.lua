return Def.ActorFrame{
    Name="Root",
    Def.Quad{Name="BounceBegin", InitCommand=cmd(zoomto,64,32;xy,100,100), OnCommand=cmd(queuecommand,"Start"), StartCommand=cmd(bouncebegin,0.5;x,200)},
    Def.Quad{Name="BounceEnd", InitCommand=cmd(zoomto,64,32;xy,100,100), OnCommand=cmd(queuecommand,"Start"), StartCommand=cmd(bounceend,0.5;x,200)},
    Def.Quad{Name="Cubic", InitCommand=cmd(zoomto,64,32;xy,100,100), OnCommand=cmd(queuecommand,"Start"), StartCommand=function(self) self:tween(0.5,"TweenType_Bezier",{0,0,1,1}):x(200) end},
    Def.ActorFrame{
        Name="QueueFrame", OnCommand=cmd(sleep,0.05;queuecommand,"Hide"),
        Def.Quad{Name="QueuedChild", OnCommand=cmd(zoomto,64,32;xy,100,100), HideCommand=cmd(visible,false)},
    },
}
