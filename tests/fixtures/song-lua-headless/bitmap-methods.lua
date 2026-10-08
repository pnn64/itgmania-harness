local function check_bitmap(native_label)
assert(type(BitmapText.GetX)=="function")
assert(native_label.GetChild==nil and native_label.SetUpdateFunction==nil and native_label.GetDrawFunction==nil)
assert(native_label.UnknownNativeMethod==nil and BitmapText.UnknownNativeMethod==nil)
assert(native_label:GetText()=="")
assert(native_label:x(13)==native_label and native_label:GetX()==13)
assert(native_label:jitter(true)==native_label and native_label:jitter(false)==native_label)
BitmapText.NativeAdded = function(self) return self:GetText() end
assert(native_label:NativeAdded()=="")
BitmapText.NativeAdded=nil
local original=Actor.GetX
Actor.GetX=function() return 91 end
local inherited=native_label:GetX()
Actor.GetX=original
assert(inherited==91 and native_label:GetX()==13)
assert(native_label:get_mult_attrs_with_diffuse()==false)
assert(native_label:set_mult_attrs_with_diffuse(true)==native_label)
assert(native_label:get_mult_attrs_with_diffuse()==true)
assert(native_label:set_mult_attrs_with_diffuse(0)==native_label)
assert(native_label:get_mult_attrs_with_diffuse()==true)
assert(native_label:set_mult_attrs_with_diffuse(false)==native_label)
assert(native_label:get_mult_attrs_with_diffuse()==false)
assert(native_label:set_mult_attrs_with_diffuse("false")==native_label)
assert(native_label:get_mult_attrs_with_diffuse()==true)
assert(native_label:set_mult_attrs_with_diffuse()==native_label)
assert(native_label:get_mult_attrs_with_diffuse()==false)
assert(native_label:set_mult_attrs_with_diffuse({})==native_label)
assert(native_label:get_mult_attrs_with_diffuse()==true)
end

return Def.ActorFrame{
 OnCommand=function(self) check_bitmap(self:GetChild("Label")) end,
 Def.BitmapText{Name="Label",Font="Common Normal",Text=""},
}
