local expected = {wrapwidthpixels=true,maxwidth=true,maxheight=true,max_dimension_use_zoom=true,vertspacing=true,settext=true,rainbowscroll=true,jitter=true,distort=true,undistort=true,GetText=true,AddAttribute=true,ClearAttributes=true,strokecolor=true,getstrokecolor=true,uppercase=true,textglowmode=true,get_mult_attrs_with_diffuse=true,set_mult_attrs_with_diffuse=true,PixelFont=true,Stroke=true,NoStroke=true,settextf=true,DiffuseAndStroke=true}
local own = {}
for name, method in pairs(BitmapText) do
 if type(method)=="function" then assert(expected[name], "unexpected own BitmapText method: "..name); own[name]=true end
end
for name in pairs(expected) do assert(own[name], "missing own BitmapText method: "..name) end
local own_sprite = rawget(Sprite, "GetNumStates")
assert(type(own_sprite)=="function", "Sprite owns GetNumStates")
local original = Actor.GetNumStates
Actor.GetNumStates = function() return 91 end
local method = Sprite.GetNumStates
Actor.GetNumStates = original
assert(method == own_sprite, "native own method shadows the overridden base")

return Def.ActorFrame{}
