for _, value in ipairs({false, 0, "class value", {}}) do
 Actor.NativeBaseValue=value
 assert(BitmapText.NativeBaseValue==value, "inherited public class value")
 Actor.NativeBaseValue=nil
end

return Def.ActorFrame{}
