// Runtime scaffolding for the ITGmania source subset compiled by this harness.
// These definitions satisfy application-level singletons and unrelated link
// symbols expected by the native loaders. Parsing, timing, note, radar, and
// tech results still come from the compiled ITGmania sources.

#include "runtime_stubs.h"

extern "C" {
#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
}

#include "global.h"
#include "Attack.h"
#include "GameManager.h"
#include "GameState.h"
#include "CommonMetrics.h"
#include "Course.h"
#include "Trail.h"
#include "MessageManager.h"
#include "PrefsManager.h"
#include "ProfileManager.h"
#include "SongManager.h"
#include "RageFile.h"
#include "RageFileDriverMemory.h"
#include "RageFileManager.h"
#include "RageLog.h"
#include "RageTexture.h"
#include "RageTextureID.h"
#include "RageTextureManager.h"
#include "ScreenMessage.h"
#include "Song.h"
#include "Style.h"
#include "ThemeManager.h"
#include "BackgroundUtil.h"
#include "ActorUtil.h"
#include "DisplaySpec.h"
#include "GameLoop.h"
#include "ImageCache.h"
#include "LightsManager.h"
#include "Sprite.h"
#include "LocalizedString.h"
#include "TechCounts.h"
#include "RadarValues.h"
#include "CryptManager.h"
#include "LuaBinding.h"
#include "LuaReference.h"
#include "LuaManager.h"
#include "EnumHelper.h"
#include "IniFile.h"
#include "NotesLoaderDWI.h"
#include "NotesLoaderKSF.h"
#include "NotesLoaderBMS.h"
#include "RageSoundReader_FileReader.h"
#include "RageTypes.h"
#include "RageUtil.h"
#include "RageSurface.h"
#include "RageSurfaceUtils_Zoom.h"
#include "RageSurface_Save_BMP.h"
#include "RageSurface_Save_JPEG.h"
#include "RageSurface_Save_PNG.h"
#include "RageUtil/Regex.h"
#include "arch/ArchHooks/ArchHooks.h"
#include "arch/Dialog/Dialog.h"
#include "arch/Threads/Threads.h"
#include "StdString.h"

#if defined(_WIN32)
#include <windows.h>
#include <bcrypt.h>
#else
#include <tomcrypt.h>
#endif

#include <array>
#include <algorithm>
#include <cctype>
#include <cerrno>
#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwctype>
#include <filesystem>
#include <fstream>
#include <limits>
#include <map>
#include <memory>
#include <new>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>
#include <ctime>
#include <cstdlib>

using RString = std::string;

template<> std::string Luna<Song>::m_sClassName = "Song";
template<> std::string Luna<Course>::m_sClassName = "Course";
template<> std::string Luna<Trail>::m_sClassName = "Trail";
ThemeMetric<std::string> CommonMetrics::DEFAULT_NOTESKIN_NAME("Common", "DefaultNoteSkinName");
ThemeMetric<std::string> CommonMetrics::DEFAULT_MODIFIERS("Common", "DefaultModifiers");

std::string CommonMetrics::LocalizeOptionItem(const std::string& name, bool optional) {
	if (optional && !THEME->HasString("OptionNames", name)) return name;
	return THEME->GetString("OptionNames", name);
}


// The semantic harness runs songs, not courses. Fail explicitly if an option
// method needs course data that this headless session cannot supply.
Trail* Course::GetTrail(StepsType, Difficulty) const {
	throw std::runtime_error("course trails are unavailable in the song harness");
}
void Trail::GetDisplayBpms(DisplayBpms&) const {
	throw std::runtime_error("course BPMs are unavailable in the song harness");
}

// ---------------------------------------------------------------------------
// Globals
namespace {

std::vector<std::string>& harness_diagnostics() {
	static std::vector<std::string> diagnostics;
	return diagnostics;
}

std::filesystem::path& harness_theme_fonts() {
	static std::filesystem::path path;
	return path;
}

std::filesystem::path& harness_fallback_fonts() {
	static std::filesystem::path path;
	return path;
}

uint32_t read_be_u32(const unsigned char* bytes) {
	return (static_cast<uint32_t>(bytes[0]) << 24) |
	       (static_cast<uint32_t>(bytes[1]) << 16) |
	       (static_cast<uint32_t>(bytes[2]) << 8) |
	       static_cast<uint32_t>(bytes[3]);
}

std::pair<int, int> jpeg_dimensions(std::ifstream& file,
                                  const std::string& path) {
	const auto invalid = [&]() {
		return std::runtime_error("invalid JPEG dimensions in texture " + path);
	};
	file.seekg(0, std::ios::end);
	const auto limit = file.tellg();
	file.seekg(2);
	// Inspect bounded header segments only. Entropy-coded image data begins
	// at SOS; dimensions must have appeared before it.
	for (size_t segment = 0; segment < 4096; ++segment) {
		if (file.get() != 0xff) throw invalid();
		int marker = file.get();
		while (marker == 0xff) marker = file.get();
		if (marker <= 0 || marker == 0xd9 || marker == 0xda) throw invalid();
		if (marker == 0x01 || (marker >= 0xd0 && marker <= 0xd7)) continue;
		std::array<unsigned char, 2> bytes{};
		if (!file.read(reinterpret_cast<char*>(bytes.data()), bytes.size())) throw invalid();
		const int length = (bytes[0] << 8) | bytes[1];
		const auto payload = file.tellg();
		if (length < 2 || payload < 0 || length - 2 > limit - payload) throw invalid();
		// SOF0/SOF2 include baseline/progressive JPEG. DHT, JPG and DAC are
		// the non-frame markers in the otherwise contiguous SOF range.
		if (marker >= 0xc0 && marker <= 0xcf && marker != 0xc4 &&
		    marker != 0xc8 && marker != 0xcc) {
			std::array<unsigned char, 6> frame{};
			if (length < 8 ||
			    !file.read(reinterpret_cast<char*>(frame.data()), frame.size())) throw invalid();
			const int height = (frame[1] << 8) | frame[2];
			const int width = (frame[3] << 8) | frame[4];
			if (width == 0 || height == 0 || frame[5] == 0 ||
			    length < 8 + 3 * frame[5]) throw invalid();
			return {width, height};
		}
		file.seekg(payload + std::streamoff(length - 2));
	}
	throw invalid();
}

std::pair<int, int> image_dimensions(const std::string& path) {
	std::ifstream file(path, std::ios::binary);
	std::array<unsigned char, 24> header{};
	if (!file.read(reinterpret_cast<char*>(header.data()), header.size())) {
		throw std::runtime_error("could not read font texture " + path);
	}
	constexpr std::array<unsigned char, 8> signature = {
		0x89, 'P', 'N', 'G', 0x0d, 0x0a, 0x1a, 0x0a};
	if (header[0] == 0xff && header[1] == 0xd8) {
		return jpeg_dimensions(file, path);
	}
	if (!std::equal(signature.begin(), signature.end(), header.begin())) {
		throw std::runtime_error(
			"headless texture probing supports PNG and JPEG headers only: " + path);
	}
	const uint32_t width = read_be_u32(header.data() + 16);
	const uint32_t height = read_be_u32(header.data() + 20);
	if (width == 0 || height == 0 ||
	    width > static_cast<uint32_t>(std::numeric_limits<int>::max()) ||
	    height > static_cast<uint32_t>(std::numeric_limits<int>::max())) {
		throw std::runtime_error("invalid PNG dimensions in font texture " + path);
	}
	return {static_cast<int>(width), static_cast<int>(height)};
}

void apply_resolution_hint(const std::string& path, int& width, int& height) {
	static Regex resolution("\\([^\\)]*res ([0-9]+)x([0-9]+).*\\)");
	std::vector<std::string> matches;
	if (!resolution.Compare(path, matches)) {
		return;
	}
	const int hinted_width = StringToInt(matches[0]);
	const int hinted_height = StringToInt(matches[1]);
	if (hinted_width > 0 && hinted_height > 0) {
		width = hinted_width;
		height = hinted_height;
	}
}

class HarnessTexture final : public RageTexture {
 public:
	explicit HarnessTexture(const RageTextureID& id) : RageTexture(id) {
		const bool synthetic = id.filename.rfind("__harness_", 0) == 0;
		const auto [width, height] = synthetic
		    ? std::pair<int, int>{64, 64}
		    : image_dimensions(id.filename);
		m_iSourceWidth = width;
		m_iSourceHeight = height;
		m_iImageWidth = width;
		m_iImageHeight = height;

		std::string hints = id.filename + id.AdditionalTextureHints;
		MakeLower(hints);
		const bool double_res = hints.find("doubleres") != std::string::npos;
		if (double_res && !TEXTUREMAN->GetPrefs().m_bHighResolutionTextures) {
			m_iImageWidth /= 2;
			m_iImageHeight /= 2;
		}
		m_iImageWidth = std::min(m_iImageWidth, id.iMaxSize);
		m_iImageHeight = std::min(m_iImageHeight, id.iMaxSize);
		m_iTextureWidth = power_of_two(m_iImageWidth);
		m_iTextureHeight = power_of_two(m_iImageHeight);
		const bool stretch = hints.find("stretch") != std::string::npos ||
		                     m_iTextureWidth < 8 || m_iTextureHeight < 8;
		m_iTextureWidth = std::max(8, m_iTextureWidth);
		m_iTextureHeight = std::max(8, m_iTextureHeight);
		if (stretch) {
			m_iImageWidth = m_iTextureWidth;
			m_iImageHeight = m_iTextureHeight;
		}

		CreateFrameRects();
		apply_resolution_hint(id.filename, m_iSourceWidth, m_iSourceHeight);
		if (double_res) {
			m_iSourceWidth /= 2;
			m_iSourceHeight /= 2;
		}
	}

	uintptr_t GetTexHandle() const override { return 0; }
};

} // namespace

static RageLog gLog;
static RageTextureManager gTextureManager;
static GameState gGameState;
static LuaManager gLuaManager;
static GameManager gGameManager;
static ThemeManager gThemeManager;
static RageFileManager gFileManager("");
static SongManager gSongManager;

RageLog* LOG = &gLog;
RageTextureManager* TEXTUREMAN = &gTextureManager;
GameState* GAMESTATE = &gGameState;
GameManager* GAMEMAN = &gGameManager;
PrefsManager* PREFSMAN = nullptr;
ThemeManager* THEME = &gThemeManager;
RageFileManager* FILEMAN = &gFileManager;
LuaManager* LUA = &gLuaManager;
SongManager* SONGMAN = &gSongManager;
ProfileManager* PROFILEMAN = nullptr;
ImageCache* IMAGECACHE = nullptr;
ArchHooks* HOOKS = nullptr;
// ScreenMessage constants provided by real ScreenMessage.cpp now.

void harness_configure_font_paths(const std::string& theme_fonts,
                                  const std::string& fallback_fonts) {
	harness_theme_fonts() = std::filesystem::path(theme_fonts);
	harness_fallback_fonts() = std::filesystem::path(fallback_fonts);
}

void harness_clear_diagnostics() { harness_diagnostics().clear(); }

std::mutex& harness_native_mutex() {
    static std::mutex mutex;
    // Initialize after all native class registrations, and destroy before the
    // shared Lua state. QueueMessage uses the linked MessageManager dispatch.
    static auto messages = [] {
        LUA->RegisterTypes();
        auto manager = std::make_unique<MessageManager>();
        MESSAGEMAN = manager.get();
        return manager;
    }();
    return mutex;
}

std::vector<std::string> harness_take_diagnostics() {
	std::vector<std::string> result;
	result.swap(harness_diagnostics());
	return result;
}

// ---------------------------------------------------------------------------
// Headless crash reporting: retain native failure semantics without opening a UI.
namespace CrashHandler {
void ForceCrash(char const* reason) {
    std::fprintf(stderr, "ITGmania fatal assertion: %s\n", reason ? reason : "unknown");
    std::fflush(stderr);
    std::_Exit(EXIT_FAILURE);
}
void ForceDeadlock(std::string, uint64_t) {}
} // namespace CrashHandler

// ---------------------------------------------------------------------------
// my_localtime_r stub (fixes unresolved my_localtime_r on Windows/MSVC)
tm* my_localtime_r(time_t const* t, tm* out) {
	if (!t || !out) return nullptr;
#if defined(_WIN32)
	// localtime_s returns errno_t
	if (localtime_s(out, t) != 0) return nullptr;
	return out;
#else
	return localtime_r(t, out);
#endif
}

// ---------------------------------------------------------------------------
// RageLog (no-op)
RageLog::RageLog() = default;
RageLog::~RageLog() = default;

void RageLog::Trace(const char* format, ...) {
	std::array<char, 4096> buffer{};
	va_list args;
	va_start(args, format);
	std::vsnprintf(buffer.data(), buffer.size(), format, args);
	va_end(args);
	if (std::strstr(buffer.data(), "Exception:") != nullptr) {
		std::fputs(buffer.data(), stderr);
		std::fputc('\n', stderr);
	}
}
void RageLog::Warn(const char* format, ...) {
	std::array<char, 4096> buffer{};
	va_list args;
	va_start(args, format);
	std::vsnprintf(buffer.data(), buffer.size(), format, args);
	va_end(args);
	harness_diagnostics().push_back(std::string("warning: ") + buffer.data());
}
void RageLog::Info(const char*, ...) {}
void RageLog::Time(const char*, ...) {}
void RageLog::UserLog(const RString& type, const RString& element, const char* format, ...) {
	std::array<char, 4096> buffer{};
	va_list args;
	va_start(args, format);
	std::vsnprintf(buffer.data(), buffer.size(), format, args);
	va_end(args);
	harness_diagnostics().push_back(type + " " + element + ": " + buffer.data());
}
void RageLog::Flush() {}
void RageLog::MapLog(const RString&, const char*, ...) {}
void RageLog::UnmapLog(const RString&) {}
const char* RageLog::GetAdditionalLog() { return nullptr; }
const char* RageLog::GetInfo() { return nullptr; }
const char* RageLog::GetRecentLog(int) { return nullptr; }
void RageLog::SetShowLogOutput(bool) {}
void RageLog::SetLogToDisk(bool) {}
void RageLog::SetInfoToDisk(bool) {}
void RageLog::SetUserLogToDisk(bool) {}
void RageLog::SetFlushing(bool) {}
void ShowWarningOrTrace(const char*, int, const RString&, bool) {}
void ShowWarningOrTrace(const char*, int, const char*, bool) {}

// ---------------------------------------------------------------------------
// Headless texture metadata used by ITGmania's FontPage calculations.
RageTextureManager::RageTextureManager()
	: m_Prefs(),
	  m_iNoWarnAboutOddDimensions(0),
	  m_TexturePolicy(RageTextureID::TEX_DEFAULT) {}
RageTextureManager::~RageTextureManager() = default;
void RageTextureManager::Update(float) {}
// Metadata-only registry owned by the serialized native harness. Live source
// and render-target handles share native RageTextureID equality; unloading the
// last reference removes an entry before its display allocation is destroyed.
static std::map<RageTextureID, RageTexture*>& harness_textures() {
	static std::map<RageTextureID, RageTexture*> textures;
	return textures;
}
RageTexture* RageTextureManager::LoadTexture(RageTextureID id) {
	return LoadTextureInternal(id);
}
RageTexture* RageTextureManager::CopyTexture(RageTexture* texture) {
	if (texture) ++texture->m_iRefCount;
	return texture;
}
bool RageTextureManager::IsTextureRegistered(RageTextureID id) const {
	AdjustTextureID(id);
	return harness_textures().find(id) != harness_textures().end();
}
void RageTextureManager::RegisterTexture(RageTextureID id, RageTexture* texture) {
	AdjustTextureID(id);
	if (!harness_textures().emplace(id, texture).second)
		throw std::runtime_error("Custom texture already registered: " + id.filename);
}
void RageTextureManager::VolatileTexture(RageTextureID) {}
void RageTextureManager::UnloadTexture(RageTexture* texture) {
	if (!texture) return;
	if (--texture->m_iRefCount == 0) DeleteTexture(texture);
}
void RageTextureManager::ReloadAll() {}
void RageTextureManager::RegisterTextureForUpdating(RageTextureID, RageTexture*) {}
bool RageTextureManager::SetPrefs(RageTextureManagerPrefs prefs) {
	m_Prefs = prefs;
	return true;
}
void RageTextureManager::DeleteTexture(RageTexture* texture) {
	harness_textures().erase(texture->GetID());
	delete texture;
}
void RageTextureManager::GarbageCollect(GCType) {}
RageTexture* RageTextureManager::LoadTextureInternal(RageTextureID id) {
	AdjustTextureID(id);
	const auto found = harness_textures().find(id);
	if (found != harness_textures().end()) return CopyTexture(found->second);
	auto* texture = new HarnessTexture(id);
	RegisterTexture(id, texture);
	return texture;
}
void RageTextureManager::InvalidateTextures() {}
void RageTextureManager::AdjustTextureID(RageTextureID&) const {}
void RageTextureManager::DiagnosticOutput() const {}
RageTextureID RageTextureManager::GetDefaultTextureID() {
	return RageTextureID("__harness_missing_texture__.png");
}
RageTextureID RageTextureManager::GetScreenTextureID() {
	return RageTextureID("__harness_screen_texture__.png");
}
RageSurface* RageTextureManager::GetScreenSurface() { return nullptr; }

namespace Dialog {
void Init() {}
void Shutdown() {}
void SetWindowed(bool) {}
void Error(std::string message, std::string) {
	harness_diagnostics().push_back(std::move(message));
}
void OK(std::string message, std::string) {
	harness_diagnostics().push_back(std::move(message));
}
Result OKCancel(std::string message, std::string) {
	harness_diagnostics().push_back(std::move(message));
	return ok;
}
Result AbortRetryIgnore(std::string message, std::string) {
	harness_diagnostics().push_back(std::move(message));
	return ignore;
}
Result AbortRetry(std::string message, std::string) {
	harness_diagnostics().push_back(std::move(message));
	return abort;
}
Result YesNo(std::string, std::string) { return no; }
void IgnoreMessage(std::string) {}
} // namespace Dialog

// ---------------------------------------------------------------------------
// Misc standalone helpers
static RString make_rstring(const char* s) { return RString(s); }
InstrumentTrack StringToInstrumentTrack(const RString& in) {
	RString s(in);
	MakeLower(s);
	if (s == "guitar") return InstrumentTrack_Guitar;
	if (s == "rhythm") return InstrumentTrack_Rhythm;
	if (s == "bass") return InstrumentTrack_Bass;
	return InstrumentTrack_Invalid;
}
const RString& InstrumentTrackToString(InstrumentTrack track) {
	static RString guitar = make_rstring("Guitar");
	static RString rhythm = make_rstring("Rhythm");
	static RString bass = make_rstring("Bass");
	static RString invalid;
	switch (track) {
		case InstrumentTrack_Guitar: return guitar;
		case InstrumentTrack_Rhythm: return rhythm;
		case InstrumentTrack_Bass: return bass;
		default: return invalid;
	}
}

LocalizedString::LocalizedString(const RString&, const RString&) : m_pImpl(nullptr) {}
LocalizedString::LocalizedString(LocalizedString const&) : m_pImpl(nullptr) {}
LocalizedString::~LocalizedString() = default;
void LocalizedString::Load(const RString&, const RString&) {}
const RString& LocalizedString::GetValue() const { static RString empty; return empty; }
void LocalizedString::RegisterLocalizer(MakeLocalizer) {}
void LocalizedString::CreateImpl() {}

// ---------------------------------------------------------------------------
// Basic string helpers
#ifndef ITGMANIA_HARNESS_SOURCE
void MakeLower(char* p, size_t len) {
	if (!p) return;
	for (size_t i = 0; i < len && p[i]; ++i) {
		p[i] = static_cast<char>(std::tolower(static_cast<unsigned char>(p[i])));
	}
}
void MakeLower(wchar_t* p, size_t len) {
	if (!p) return;
	for (size_t i = 0; i < len && p[i]; ++i) {
		p[i] = static_cast<wchar_t>(std::towlower(p[i]));
	}
}
namespace StdString {
void ssasn(std::string& dst, const std::string& src) noexcept { dst = src; }
void ssasn(std::wstring& dst, const std::wstring& src) noexcept { dst = src; }
void ssasn(std::string& dst, const char* src) noexcept { dst = src ? src : ""; }
void ssasn(std::wstring& dst, const wchar_t* src) noexcept { dst = src ? src : L""; }
char sstolower(char ch) noexcept { return (ch >= 'A' && ch <= 'Z') ? static_cast<char>(ch + 'a' - 'A') : ch; }
wchar_t sstolower(wchar_t ch) noexcept { return (ch >= L'A' && ch <= L'Z') ? static_cast<wchar_t>(ch + L'a' - L'A') : ch; }
} // namespace StdString
#endif

RadarValues::RadarValues() { Zero(); }
void RadarValues::MakeUnknown() { FOREACH_ENUM(RadarCategory, rc) m_Values[rc] = RADAR_VAL_UNKNOWN; }
void RadarValues::Zero() { FOREACH_ENUM(RadarCategory, rc) m_Values[rc] = 0.0f; }
XNode* RadarValues::CreateNode(bool, bool) const { return nullptr; }
void RadarValues::LoadFromNode(const XNode*) {}
RString RadarValues::ToString(int) const { return ""; }
void RadarValues::FromString(RString) {}
ThemeMetric<bool> RadarValues::WRITE_SIMPLE_VALIES("", "");
ThemeMetric<bool> RadarValues::WRITE_COMPLEX_VALIES("", "");
void RadarValues::PushSelf(lua_State*) {}

#ifndef ITGMANIA_HARNESS_SOURCE
TechCounts::TechCounts() { Zero(); }
void TechCounts::MakeUnknown() { FOREACH_ENUM(TechCountsCategory, tc) m_Values[tc] = TECHCOUNTS_VAL_UNKNOWN; }
void TechCounts::Zero() { FOREACH_ENUM(TechCountsCategory, tc) m_Values[tc] = 0.0f; }
RString TechCounts::ToString(int) const { return ""; }
void TechCounts::FromString(RString) {}
void TechCounts::PushSelf(lua_State*) {}
void TechCounts::CalculateTechCountsFromRows(const std::vector<StepParity::Row>&, StepParity::StageLayout&, TechCounts& out) { out.Zero(); }
#endif

void Attack::GetAttackBeats(const Song*, float& fStartBeat, float& fEndBeat) const { fStartBeat = 0.0f; fEndBeat = 0.0f; }
void Attack::GetRealtimeAttackBeats(const Song*, const PlayerState*, float& fStartBeat, float& fEndBeat) const { fStartBeat = 0.0f; fEndBeat = 0.0f; }
bool Attack::operator==(const Attack& rhs) const { return sModifiers == rhs.sModifiers && fStartSecond == rhs.fStartSecond && fSecsRemaining == rhs.fSecsRemaining; }
bool Attack::ContainsTransformOrTurn() const { return false; }
Attack Attack::FromGlobalCourseModifier(const RString& mods) { return Attack(ATTACK_LEVEL_1, ATTACK_STARTS_NOW, 0.0f, mods, false, true); }
RString Attack::GetTextDescription() const { return sModifiers; }
int Attack::GetNumAttacks() const { return 0; }


RString CryptManager::GetSHA1ForString(RString s) {
#if defined(_WIN32)
	if (s.size() > std::numeric_limits<ULONG>::max()) {
		throw std::length_error("SHA-1 input exceeds Windows CNG limit");
	}
	BCRYPT_ALG_HANDLE algorithm = nullptr;
	if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA1_ALGORITHM, nullptr, 0) < 0) {
		throw std::runtime_error("could not open the Windows SHA-1 provider");
	}
	std::array<UCHAR, 20> digest{};
	const NTSTATUS status = BCryptHash(
		algorithm,
		nullptr,
		0,
		reinterpret_cast<PUCHAR>(s.data()),
		static_cast<ULONG>(s.size()),
		digest.data(),
		static_cast<ULONG>(digest.size()));
	BCryptCloseAlgorithmProvider(algorithm, 0);
	if (status < 0) {
		throw std::runtime_error("Windows CNG could not calculate SHA-1");
	}
	return RString(reinterpret_cast<const char*>(digest.data()), digest.size());
#else
	hash_state hash;
	std::array<unsigned char, 20> digest{};
	sha1_init(&hash);
	sha1_process(
		&hash,
		reinterpret_cast<const unsigned char*>(s.data()),
		static_cast<unsigned long>(s.size()));
	sha1_done(&hash, digest.data());
	return RString(reinterpret_cast<const char*>(digest.data()), digest.size());
#endif
}

int64_t ArchHooks::GetSystemTimeInMicroseconds() { return 0; }

// ---------------------------------------------------------------------------
// RageTimer minimal implementation
#ifndef ITGMANIA_HARNESS_SOURCE
const RageTimer RageZeroTimer;

float RageTimer::Ago() const { return 0.0f; }
void RageTimer::Touch() { m_secs = 0; m_us = 0; }
float RageTimer::GetDeltaTime() { Touch(); return 0.0f; }
double RageTimer::GetTimeSinceStart() { return 0.0; }
int RageTimer::GetTimeSinceStartSeconds() { return 0; }
uint64_t RageTimer::GetTimeSinceStartMicroseconds() { return 0; }
RageTimer RageTimer::Half() const { return *this; }
RageTimer RageTimer::operator+(float) const { return *this; }
float RageTimer::operator-(const RageTimer&) const { return 0.0f; }
bool RageTimer::operator<(const RageTimer& rhs) const {
	return m_secs < rhs.m_secs || (m_secs == rhs.m_secs && m_us < rhs.m_us);
}
#endif

int LuaHelpers::TypeError(Lua* L, int index, const char* type) {
	return luaL_typerror(L, index, type);
}

void IPreference::SetFromStack(lua_State*) {}
void IPreference::PushValue(lua_State*) const {}

// Minimal LuaManager / LuaReference scaffolding for Lua registration macros.
LuaManager::LuaManager() : m_pLuaMain(luaL_newstate()) {}
LuaManager::~LuaManager() {
	if (m_pLuaMain) {
		lua_close(m_pLuaMain);
		m_pLuaMain = nullptr;
	}
}
static std::vector<RegisterWithLuaFn>& lua_registrations() {
	static std::vector<RegisterWithLuaFn> functions;
	return functions;
}
void LuaManager::Register(RegisterWithLuaFn fn) { lua_registrations().push_back(fn); }
void harness_register_lua_globals(lua_State* state) {
	for (RegisterWithLuaFn fn : lua_registrations()) fn(state);
}
Lua* LuaManager::Get() {
	if (!m_pLuaMain) {
		m_pLuaMain = luaL_newstate();
	}
	return m_pLuaMain;
}
void LuaManager::Release(Lua*& p) { p = m_pLuaMain; }
void LuaManager::YieldLua() {}
void LuaManager::UnyieldLua() {}
void LuaManager::RegisterTypes() {
	// Semantic sessions replace the shared enum references with their own registry.
	// Refresh them even when this manager retains the same native Lua state.
	Lua* state = Get();
	harness_register_lua_globals(state);
}
void LuaManager::SetGlobal(const RString& name, int value) {
	if (!m_pLuaMain) return;
	lua_pushinteger(m_pLuaMain, value);
	lua_setglobal(m_pLuaMain, name.c_str());
}
void LuaManager::SetGlobal(const RString& name, const RString& value) {
	if (!m_pLuaMain) return;
	lua_pushlstring(m_pLuaMain, value.data(), value.size());
	lua_setglobal(m_pLuaMain, name.c_str());
}
void LuaManager::UnsetGlobal(const RString& name) {
	if (!m_pLuaMain) return;
	lua_pushnil(m_pLuaMain);
	lua_setglobal(m_pLuaMain, name.c_str());
}

LuaReference::LuaReference() : m_iReference(LUA_NOREF) {}
LuaReference::~LuaReference() = default;
LuaReference::LuaReference(const LuaReference& other) : m_iReference(LUA_NOREF) {
	if (other.m_iReference == LUA_REFNIL) {
		m_iReference = LUA_REFNIL;
	} else if (other.m_iReference != LUA_NOREF) {
		Lua* state = LUA->Get();
		other.PushSelf(state);
		m_iReference = luaL_ref(state, LUA_REGISTRYINDEX);
		LUA->Release(state);
	}
}
LuaReference& LuaReference::operator=(const LuaReference& other) {
	if (this == &other) return *this;
	m_iReference = LUA_NOREF;
	if (other.m_iReference == LUA_REFNIL) {
		m_iReference = LUA_REFNIL;
	} else if (other.m_iReference != LUA_NOREF) {
		Lua* state = LUA->Get();
		other.PushSelf(state);
		m_iReference = luaL_ref(state, LUA_REGISTRYINDEX);
		LUA->Release(state);
	}
	return *this;
}
void LuaReference::SetFromStack(Lua* state) {
	m_iReference = luaL_ref(state, LUA_REGISTRYINDEX);
}
void LuaReference::SetFromNil() { m_iReference = LUA_REFNIL; }
bool LuaReference::SetFromExpression(const RString&) { m_iReference = LUA_NOREF; return true; }
void LuaReference::DeepCopy() {}
void LuaReference::PushSelf(lua_State* L) const {
	if (!L) return;
	if (m_iReference == LUA_REFNIL) {
		lua_pushnil(L);
	} else if (m_iReference == LUA_NOREF) {
		if (dynamic_cast<const LuaClass*>(this) == nullptr) {
			lua_pushnil(L);
			return;
		}
		lua_newtable(L);
		lua_pushvalue(L, -1);
		const_cast<LuaReference*>(this)->m_iReference =
			luaL_ref(L, LUA_REGISTRYINDEX);
	} else {
		lua_rawgeti(L, LUA_REGISTRYINDEX, m_iReference);
	}
}
bool LuaReference::IsSet() const { return m_iReference != LUA_NOREF; }
bool LuaReference::IsNil() const { return m_iReference == LUA_REFNIL; }
int LuaReference::GetLuaType() const { return IsSet() ? LUA_TNIL : LUA_TNONE; }
RString LuaReference::Serialize() const { return ""; }
void LuaReference::Unregister() { m_iReference = LUA_NOREF; }
LuaTable::LuaTable() {
	Lua* state = LUA->Get();
	lua_newtable(state);
	SetFromStack(state);
	LUA->Release(state);
}
void LuaTable::Set(Lua* state, const RString& key) {
	const int top = lua_gettop(state);
	PushSelf(state);
	lua_pushvalue(state, top);
	lua_setfield(state, -2, key.c_str());
	lua_settop(state, top - 1);
}
void LuaTable::Get(Lua* state, const RString& key) {
	PushSelf(state);
	lua_getfield(state, -1, key.c_str());
	lua_remove(state, -2);
}


template<> LuaReference EnumTraits<PlayerNumber>::StringToEnum;
template<> LuaReference EnumTraits<PlayerNumber>::EnumToString;
template<> PlayerNumber EnumTraits<PlayerNumber>::Invalid = PLAYER_INVALID;
template<> const char* EnumTraits<PlayerNumber>::szName = "PlayerNumber";

namespace LuaHelpers {
Dialog::Result ReportScriptError(
	const std::string& error, std::string, bool use_abort) {
	static bool reporting = false;
	if (!reporting) {
		reporting = true;
		ScriptErrorMessage(error);
		reporting = false;
	}
	harness_diagnostics().push_back(error);
	return use_abort ? Dialog::ignore : Dialog::ok;
}
void ScriptErrorMessage(const std::string& error) {
	Message message("ScriptError");
	message.SetParam("message", error);
	if (MESSAGEMAN != nullptr) MESSAGEMAN->Broadcast(message);
}
void ReportScriptErrorFmt(const char* format, ...) {
	std::array<char, 4096> buffer{};
	va_list args;
	va_start(args, format);
	std::vsnprintf(buffer.data(), buffer.size(), format, args);
	va_end(args);
	ReportScriptError(buffer.data());
}
template<> void Push<float>(lua_State* L, float const& v) { lua_pushnumber(L, v); }
template<> void Push<bool>(lua_State* L, bool const& v) { lua_pushboolean(L, v); }
template<> void Push<int>(lua_State* L, int const& v) { lua_pushinteger(L, v); }
template<> void Push<double>(lua_State* L, double const& v) { lua_pushnumber(L, v); }
template<> void Push<PlayerNumber>(lua_State* L, PlayerNumber const& v) { lua_pushinteger(L, v); }
template<> void Push<GameController>(lua_State* L, GameController const& v) { lua_pushinteger(L, v); }
template<> void Push<std::string>(lua_State* L, std::string const& v) { lua_pushlstring(L, v.data(), v.size()); }

template<> bool FromStack<float>(lua_State* L, float& out, int i) { out = static_cast<float>(lua_tonumber(L, i)); return true; }
template<> bool FromStack<bool>(lua_State* L, bool& out, int i) { out = lua_toboolean(L, i) != 0; return true; }
template<> bool FromStack<int>(lua_State* L, int& out, int i) { out = static_cast<int>(lua_tointeger(L, i)); return true; }
template<> bool FromStack<std::string>(lua_State* L, std::string& out, int i) { size_t len = 0; const char* s = lua_tolstring(L, i, &len); out.assign(s ? s : "", len); return true; }

void PushValueFunc(lua_State* L, int args) {
	// LuaManager::PushValueFunc: capture arguments as constant return values.
	const int top = lua_gettop(L) - args + 1;
	lua_pushinteger(L, args);
	lua_insert(L, top);
	lua_pushcclosure(L, [](lua_State* L) -> int {
		const int count = static_cast<int>(lua_tointeger(L, lua_upvalueindex(1)));
		for (int i = 0; i < count; ++i) lua_pushvalue(L, lua_upvalueindex(i + 2));
		return count;
	}, args + 1);
}
bool RunScriptOnStack(Lua*, std::string&, int, int, bool) { return false; }
bool RunScript(
	Lua*, const std::string&, const std::string&, std::string&, int, int, bool) {
	return false;
}
bool RunExpression(Lua*, const RString&, const RString&) { return false; }
void ParseCommandList(lua_State*, const std::string&, const std::string&, bool) {}
void DeepCopy(lua_State*) {}

void CreateTableFromArrayB(Lua* L, const std::vector<bool>& vals) {
	lua_newtable(L);
	int idx = 1;
	for (bool v : vals) {
		lua_pushboolean(L, v);
		lua_rawseti(L, -2, idx++);
	}
}
} // namespace LuaHelpers

LuaThreadVariable::LuaThreadVariable(const std::string&, const std::string&)
	: m_Name(nullptr), m_pOldValue(nullptr) {}
LuaThreadVariable::LuaThreadVariable(const std::string&, const LuaReference&)
	: m_Name(nullptr), m_pOldValue(nullptr) {}
LuaThreadVariable::~LuaThreadVariable() = default;

namespace ActorUtil {
apActorCommands ParseActorCommands(const std::string&, const std::string&) {
	return apActorCommands();
}
Actor* LoadFromNode(const XNode*, Actor*) { return nullptr; }
bool LoadTableFromStackShowErrors(Lua*) { return false; }
void Register(const std::string&, CreateActorFn) {}
Actor* MakeActor(const std::string&, Actor*) { return nullptr; }
std::string GetWhere(const XNode*) { return {}; }
bool GetAttrPath(const XNode*, const std::string&, std::string&, bool) {
	return false;
}
FileType GetFileType(const std::string&) { return FT_Bitmap; }
void SortByZPosition(std::vector<Actor*>& actors) {
	std::stable_sort(
		actors.begin(), actors.end(),
		[](const Actor* left, const Actor* right) {
			return left->GetZ() < right->GetZ();
		});
}
} // namespace ActorUtil

bool DoesFileExist(const RString& path) { return FILEMAN && FILEMAN->DoesFileExist(path); }
bool IsAFile(const RString& path) { return FILEMAN && FILEMAN->IsAFile(path); }
bool IsADirectory(const RString& path) { return FILEMAN && FILEMAN->IsADirectory(path); }
void GetDirListing(const RString& path, std::vector<RString>& out, bool onlyDirs, bool returnPathToo) {
	if (FILEMAN) FILEMAN->GetDirListing(path, out, onlyDirs, returnPathToo);
	else out.clear();
}

RageSoundReader_FileReader* RageSoundReader_FileReader::OpenFile(RString, RString& error, bool*) { error = ""; return nullptr; }

ThreadImpl* MakeThread(int (*)(void*), void*, uint64_t* piThreadID) { if (piThreadID) *piThreadID = 0; return nullptr; }
ThreadImpl* MakeThisThread() { return nullptr; }
MutexImpl* MakeMutex(RageMutex*) { return nullptr; }
EventImpl* MakeEvent(MutexImpl*) { return nullptr; }
SemaImpl* MakeSemaphore(int) { return nullptr; }
uint64_t GetThisThreadId() { return 0; }
uint64_t GetInvalidThreadId() { return 0; }

namespace GameLoop {
float GetUpdateRate() { return 1.0f; }
}

CabinetLight StringToCabinetLight(const std::string&) {
	return CabinetLight_Invalid;
}

void my_usleep(unsigned long) {}

DisplaySpecs* pushDisplaySpecs(lua_State*, const DisplaySpecs&) {
	return nullptr;
}

namespace RageSurfaceUtils {
void Zoom(RageSurface*&, int, int) {}
bool SaveBMP(RageSurface*, RageFile&) { return false; }
bool SaveJPEG(RageSurface*, RageFile&, bool) { return false; }
bool SavePNG(RageSurface*, RageFile&, std::string&) { return false; }
} // namespace RageSurfaceUtils

RageTextureID ImageCache::LoadCachedImage(std::string, std::string path) {
	return RageTextureID(path);
}

// ---------------------------------------------------------------------------
// Minimal RageFile backed by std::ifstream
class RageFileStd final : public RageFileBasic {
  public:
	RageFileStd() : m_mode(0), m_size(-1) {}
	explicit RageFileStd(const RString& path, int mode = 0)
		: m_path(path), m_mode(mode), m_size(-1) {}

	RageFileBasic* Copy() const override {
		auto* copy = new RageFileStd(m_path, m_mode);
		if (!m_path.empty() && copy->Open(m_path, m_mode)) {
			const int pos = Tell();
			if (pos >= 0) copy->Seek(pos);
		}
		return copy;
	}
	RString GetDisplayPath() const override { return m_path; }
	RString GetError() const override { return m_error; }
	void ClearError() override { m_error.clear(); }
	bool AtEOF() const override { return !m_stream || ((m_mode & RageFile::READ) && m_stream->eof()); }

	int Seek(int offset) override {
		if (!m_stream) return -1;
		m_stream->clear();
		if (m_mode & RageFile::READ) {
			m_stream->seekg(offset, std::ios::beg);
		}
		if (m_mode & RageFile::WRITE) {
			m_stream->seekp(offset, std::ios::beg);
		}
		return Tell();
	}
	int Seek(int offset, int whence) override {
		if (!m_stream) return -1;
		std::ios::seekdir dir = std::ios::beg;
		if (whence == SEEK_CUR) dir = std::ios::cur;
		else if (whence == SEEK_END) dir = std::ios::end;
		m_stream->clear();
		if (m_mode & RageFile::READ) {
			m_stream->seekg(offset, dir);
		}
		if (m_mode & RageFile::WRITE) {
			m_stream->seekp(offset, dir);
		}
		return Tell();
	}
	int Tell() const override {
		if (!m_stream) return -1;
		const std::streampos pos =
			(m_mode & RageFile::READ) ? m_stream->tellg() : m_stream->tellp();
		return (pos == std::streampos(-1)) ? -1 : static_cast<int>(pos);
	}

	int Read(void* buffer, size_t bytes) override {
		if (!m_stream || !(m_mode & RageFile::READ)) return -1;
		m_stream->read(static_cast<char*>(buffer), static_cast<std::streamsize>(bytes));
		return static_cast<int>(m_stream->gcount());
	}
	int Read(RString& buffer, int bytes = -1) override {
		if (!m_stream || !(m_mode & RageFile::READ)) return -1;
		if (bytes < 0) {
			std::ostringstream ss;
			ss << m_stream->rdbuf();
			buffer = ss.str();
			return static_cast<int>(buffer.size());
		}
		buffer.resize(bytes);
		m_stream->read(buffer.data(), bytes);
		return static_cast<int>(m_stream->gcount());
	}
	int Read(void* buffer, size_t bytes, int nmemb) override {
		const int read = Read(buffer, bytes * static_cast<size_t>(nmemb));
		if (read < 0 || bytes == 0) return read;
		return read / static_cast<int>(bytes);
	}

	int Write(const void* buffer, size_t bytes) override {
		if (!m_stream || !(m_mode & RageFile::WRITE)) return -1;
		m_stream->write(static_cast<const char*>(buffer), static_cast<std::streamsize>(bytes));
		if (!*m_stream) {
			m_error = "write failed";
			return -1;
		}
		m_size = -1;
		return 0;
	}
	int Write(const RString& s) override { return Write(s.data(), s.size()); }
	int Write(const void* buffer, size_t bytes, int nmemb) override {
		return Write(buffer, bytes * static_cast<size_t>(nmemb));
	}
	int Flush() override {
		if (!m_stream) return -1;
		m_stream->flush();
		if (!*m_stream) {
			m_error = "flush failed";
			return -1;
		}
		return 0;
	}

	int GetLine(RString& out) override {
		if (!m_stream || !(m_mode & RageFile::READ)) return -1;
		std::string line;
		if (!std::getline(*m_stream, line)) return 0;
		if (!line.empty() && line.back() == '\r') line.pop_back();
		out = line;
		return 1;
	}
	int PutLine(const RString& s) override {
		if (Write(s) == -1) return -1;
		return Write("\r\n", 2);
	}

	void EnableCRC32(bool) override {}
	bool GetCRC32(uint32_t*) override { return false; }

	int GetFileSize() const override {
		if (m_size >= 0) return m_size;
		if (!m_stream) return -1;
		if (m_mode & RageFile::WRITE) {
			m_stream->flush();
			std::error_code ec;
			const auto size = std::filesystem::file_size(m_path.c_str(), ec);
			if (ec) return -1;
			m_size = static_cast<int>(size);
			return m_size;
		}
		auto cur = m_stream->tellg();
		m_stream->seekg(0, std::ios::end);
		m_size = static_cast<int>(m_stream->tellg());
		m_stream->seekg(cur);
		return m_size;
	}
	int GetFD() override { return -1; }

	bool Open(const RString& path, int mode) {
		m_path = path;
		m_mode = mode;
		m_error.clear();
		std::ios::openmode open_mode = std::ios::binary;
		if (mode & RageFile::READ) open_mode |= std::ios::in;
		if (mode & RageFile::WRITE) {
			open_mode |= std::ios::out | std::ios::trunc;
			std::error_code ec;
			const auto parent = std::filesystem::path(path.c_str()).parent_path();
			if (!parent.empty()) {
				std::filesystem::create_directories(parent, ec);
			}
		}
		m_stream.reset(new std::fstream(path.c_str(), open_mode));
		if (!*m_stream) {
			m_error = "open failed";
			m_stream.reset();
			return false;
		}
		m_size = -1;
		return true;
	}

	  private:
		RString m_path;
		int m_mode;
		mutable int m_size;
		RString m_error;
		mutable std::unique_ptr<std::fstream> m_stream;
	};

RageFile::RageFile() : m_File(nullptr), m_Mode(0) {}
RageFile::RageFile(const RageFile& cpy) : RageFileBasic(cpy) {
	m_File = nullptr;
	m_Path = cpy.m_Path;
	m_Mode = cpy.m_Mode;
	if (cpy.m_File) {
		m_File = cpy.m_File->Copy();
	}
}
RageFile* RageFile::Copy() const { return new RageFile(*this); }
RString RageFile::GetPath() const { return m_Path; }
bool RageFile::Open(const RString& path, int mode) {
	Close();
	if ((mode & READ) && (mode & WRITE)) {
		SetError("Reading and writing are mutually exclusive");
		return false;
	}
	if (!(mode & READ) && !(mode & WRITE)) {
		SetError("Neither reading nor writing specified");
		return false;
	}
	m_Mode = mode;
	auto* f = new RageFileStd;
	if (!f->Open(path, mode)) {
		SetError(f->GetError());
		delete f;
		return false;
	}
	m_File = f;
	m_Path = path;
	return true;
}
void RageFile::Close() {
	if (m_File) {
		if (m_Mode & WRITE) {
			m_File->Flush();
		}
		delete m_File;
		m_File = nullptr;
	}
}
int RageFile::GetLine(RString& out) { return m_File ? m_File->GetLine(out) : -1; }
int RageFile::PutLine(const RString& s) { return m_File ? m_File->PutLine(s) : -1; }
void RageFile::EnableCRC32(bool on) { if (m_File) m_File->EnableCRC32(on); }
bool RageFile::GetCRC32(uint32_t* out) { return m_File && m_File->GetCRC32(out); }
int RageFile::Read(void* buffer, size_t bytes) { return m_File ? m_File->Read(buffer, bytes) : -1; }
int RageFile::Read(RString& buffer, int bytes) { return m_File ? m_File->Read(buffer, bytes) : -1; }
int RageFile::Read(void* buffer, size_t bytes, int nmemb) {
	return m_File ? m_File->Read(buffer, bytes, nmemb) : -1;
}
int RageFile::Write(const void* buffer, size_t bytes) { return m_File ? m_File->Write(buffer, bytes) : -1; }
int RageFile::Write(const void* buffer, size_t bytes, int nmemb) {
	return m_File ? m_File->Write(buffer, bytes, nmemb) : -1;
}
int RageFile::Flush() { return m_File ? m_File->Flush() : -1; }
int RageFile::Seek(int offset) { return m_File ? m_File->Seek(offset) : -1; }
int RageFile::Seek(int offset, int whence) { return m_File ? m_File->Seek(offset, whence) : -1; }
int RageFile::Tell() const { return m_File ? m_File->Tell() : -1; }
int RageFile::GetFileSize() const { return m_File ? m_File->GetFileSize() : -1; }
int RageFile::GetFD() { return -1; }
RString RageFile::GetError() const { return m_sError; }
void RageFile::ClearError() { m_sError = ""; }
bool RageFile::AtEOF() const { return m_File ? m_File->AtEOF() : true; }
void RageFile::SetError(const RString& err) { m_sError = err; }
void RageFile::PushSelf(lua_State*) {}

// ---------------------------------------------------------------------------
// RageFileManager (std::filesystem backed)
namespace {

bool wildcard_match(std::string pattern, std::string value) {
	MakeLower(pattern);
	MakeLower(value);
	size_t pattern_index = 0;
	size_t value_index = 0;
	size_t star_index = std::string::npos;
	size_t star_value_index = 0;
	while (value_index < value.size()) {
		if (pattern_index < pattern.size() &&
		    (pattern[pattern_index] == '?' ||
		     pattern[pattern_index] == value[value_index])) {
			++pattern_index;
			++value_index;
		} else if (pattern_index < pattern.size() && pattern[pattern_index] == '*') {
			star_index = pattern_index++;
			star_value_index = value_index;
		} else if (star_index != std::string::npos) {
			pattern_index = star_index + 1;
			value_index = ++star_value_index;
		} else {
			return false;
		}
	}
	while (pattern_index < pattern.size() && pattern[pattern_index] == '*') {
		++pattern_index;
	}
	return pattern_index == pattern.size();
}

void harness_dir_listing(
	const RString& raw_path, std::vector<RString>& out, bool only_dirs,
	bool return_path) {
	out.clear();
	std::filesystem::path input(raw_path);
	std::error_code ec;
	if (std::filesystem::is_directory(input, ec)) {
		input /= "*";
	}
	const std::filesystem::path directory =
		input.parent_path().empty() ? std::filesystem::path(".") : input.parent_path();
	const std::string pattern = input.filename().string();
	for (const auto& entry : std::filesystem::directory_iterator(directory, ec)) {
		if (ec) break;
		if (only_dirs && !entry.is_directory()) continue;
		if (!wildcard_match(pattern, entry.path().filename().string())) continue;
		out.push_back(return_path ? entry.path().generic_string()
		                          : entry.path().filename().generic_string());
	}
	std::sort(out.begin(), out.end(), [](const RString& left, const RString& right) {
		return CompareNoCase(left, right) < 0;
	});
}

} // namespace

RageFileManager::RageFileManager(const RString&) {}
RageFileManager::~RageFileManager() {}
void RageFileManager::MountInitialFilesystems() {}
void RageFileManager::MountUserFilesystems() {}
void RageFileManager::GetDirListing(const RString& path, std::vector<RString>& out, bool onlyDirs, bool returnPathToo) {
	harness_dir_listing(path, out, onlyDirs, returnPathToo);
}
void RageFileManager::GetDirListingWithMultipleExtensions(
	const RString& path,
	std::vector<RString> const& exts,
	std::vector<RString>& out,
	bool onlyDirs,
	bool returnPathToo)
{
	std::vector<RString> candidates;
	harness_dir_listing(path, candidates, onlyDirs, true);
	out.clear();
	for (const RString& candidate : candidates) {
		std::string extension = std::filesystem::path(candidate).extension().string();
		if (!extension.empty() && extension.front() == '.') extension.erase(0, 1);
		const bool matches = exts.empty() || std::any_of(
			exts.begin(), exts.end(), [&](const RString& expected) {
				return CompareNoCase(extension, expected) == 0;
			});
		if (!matches) continue;
		out.push_back(returnPathToo
			? candidate
			: std::filesystem::path(candidate).filename().generic_string());
	}
}
bool RageFileManager::Move(const RString&, const RString&) { return false; }
bool RageFileManager::Copy(const std::string&, const std::string&) { return false; }
bool RageFileManager::Remove(const RString& path) {
	std::error_code ec;
	return std::filesystem::remove(path.c_str(), ec);
}
bool RageFileManager::DeleteRecursive(const RString&) { return false; }
void RageFileManager::CreateDir(const RString& path) {
	std::error_code ec;
	std::filesystem::create_directories(path.c_str(), ec);
}
RageFileManager::FileType RageFileManager::GetFileType(const RString& path) {
	std::error_code ec;
	auto status = std::filesystem::status(path.c_str(), ec);
	if (ec) return TYPE_NONE;
	if (std::filesystem::is_directory(status)) return TYPE_DIR;
	if (std::filesystem::is_regular_file(status)) return TYPE_FILE;
	return TYPE_NONE;
}
bool RageFileManager::IsAFile(const RString& path) { return GetFileType(path) == TYPE_FILE; }
bool RageFileManager::IsADirectory(const RString& path) { return GetFileType(path) == TYPE_DIR; }
bool RageFileManager::DoesFileExist(const RString& path) { return IsAFile(path); }
int RageFileManager::GetFileSizeInBytes(const RString& path) {
	std::error_code ec;
	auto sz = std::filesystem::file_size(path.c_str(), ec);
	return ec ? -1 : static_cast<int>(sz);
}
int RageFileManager::GetFileHash(const RString&) { return 0; }
RString RageFileManager::ResolvePath(const RString& path) { return path; }
bool RageFileManager::Mount(const RString&, const RString&, const RString&) { return true; }
void RageFileManager::Unmount(const RString&, const RString&, const RString&) {}
void RageFileManager::Remount(RString, RString) {}
bool RageFileManager::IsMounted(RString) { return true; }
void RageFileManager::GetLoadedDrivers(std::vector<DriverLocation>&) {}
void RageFileManager::FlushDirCache(const RString&) {}
RageFileBasic* RageFileManager::Open(const RString& path, int mode, int& error) {
	auto* f = new RageFileStd;
	if (!f->Open(path, mode)) {
		error = errno;
		delete f;
		return nullptr;
	}
	error = 0;
	return f;
}
void RageFileManager::CacheFile(const RageFileBasic*, const RString&) {}
RageFileDriver* RageFileManager::GetFileDriver(RString) { return nullptr; }
void RageFileManager::ReleaseFileDriver(RageFileDriver*) {}
bool RageFileManager::Unzip(const std::string&, std::string, int) { return false; }
void RageFileManager::ProtectPath(const std::string&) {}
bool RageFileManager::IsPathProtected(const std::string&) { return false; }
void RageFileManager::PushSelf(lua_State*) {}

struct RageFileObjMemFile {
	std::string value;
};

RageFileObjMem::RageFileObjMem(RageFileObjMemFile* file)
	: m_pFile(file ? file : new RageFileObjMemFile), m_iFilePos(0) {}
RageFileObjMem::RageFileObjMem(const RageFileObjMem& other)
	: RageFileObj(other), m_pFile(new RageFileObjMemFile(*other.m_pFile)),
	  m_iFilePos(other.m_iFilePos) {}
RageFileObjMem::~RageFileObjMem() { delete m_pFile; }
int RageFileObjMem::ReadInternal(void* buffer, size_t bytes) {
	const size_t position = static_cast<size_t>(m_iFilePos);
	const size_t available = position < m_pFile->value.size()
		? m_pFile->value.size() - position
		: 0;
	const size_t count = std::min(bytes, available);
	std::memcpy(buffer, m_pFile->value.data() + position, count);
	m_iFilePos += static_cast<int>(count);
	return static_cast<int>(count);
}
int RageFileObjMem::WriteInternal(const void* buffer, size_t bytes) {
	const size_t position = static_cast<size_t>(m_iFilePos);
	if (position > m_pFile->value.size()) m_pFile->value.resize(position);
	m_pFile->value.replace(position, bytes, static_cast<const char*>(buffer), bytes);
	m_iFilePos += static_cast<int>(bytes);
	return static_cast<int>(bytes);
}
int RageFileObjMem::SeekInternal(int offset) {
	m_iFilePos = std::clamp(offset, 0, GetFileSize());
	return m_iFilePos;
}
int RageFileObjMem::GetFileSize() const {
	return static_cast<int>(m_pFile->value.size());
}
RageFileObjMem* RageFileObjMem::Copy() const { return new RageFileObjMem(*this); }
const std::string& RageFileObjMem::GetString() const { return m_pFile->value; }
void RageFileObjMem::PutString(const std::string& value) { m_pFile->value = value; }

namespace RageFileManagerUtil {
RString sDirOfExecutable;
}

bool ilt(const RString& a, const RString& b) { return CompareNoCase(a, b) < 0; }
bool ieq(const RString& a, const RString& b) { return CompareNoCase(a, b) == 0; }

// ---------------------------------------------------------------------------
// Preference/IPreference stubs (avoid full Preference.cpp dependency)
IPreference::IPreference(const RString&, PreferenceType) : m_sName(""), m_bDoNotWrite(false), m_bImmutable(false) {}
IPreference::~IPreference() = default;
void IPreference::ReadFrom(const XNode*, bool) {}
void IPreference::WriteTo(XNode*) const {}
void IPreference::ReadDefaultFrom(const XNode*) {}
IPreference* IPreference::GetPreferenceByName(const RString&) { return nullptr; }
void IPreference::LoadAllDefaults() {}
void IPreference::ReadAllPrefsFromNode(const XNode*, bool) {}
void IPreference::SavePrefsToNode(XNode*) {}
void IPreference::ReadAllDefaultsFromNode(const XNode*) {}
void BroadcastPreferenceChanged(const RString&) {}

// ---------------------------------------------------------------------------
// GameState / GameManager / ThemeManager stubs
GameState::GameState()
	: masterPlayerNumber(PLAYER_1),
	  processedTiming(nullptr),
	  m_pCurGame(MessageID_Invalid),
	  m_pCurStyle(MessageID_Invalid),
	  m_PlayMode(MessageID_Invalid),
	  m_iCoins(MessageID_Invalid),
	  m_bMultiplayer(false),
	  m_iNumMultiplayerNoteFields(0),
	  m_timeGameStarted(),
	  m_Environment(nullptr),
	  m_iGameSeed(0),
	  m_iStageSeed(0),
	  m_sPreferredSongGroup(MessageID_Invalid),
	  m_sPreferredCourseGroup(MessageID_Invalid),
	  m_bFailTypeWasExplicitlySet(false),
	  m_PreferredStepsType(MessageID_Invalid),
	  m_PreferredDifficulty(MessageID_Invalid),
	  m_PreferredCourseDifficulty(MessageID_Invalid),
	  m_SortOrder(MessageID_Invalid),
	  m_PreferredSortOrder(SortOrder_Invalid),
	  m_EditMode(EditMode_Invalid),
	  m_bDemonstrationOrJukebox(false),
	  m_bJukeboxUsesModifiers(false),
	  m_iNumStagesOfThisSong(0),
	  m_iCurrentStageIndex(0),
	  m_AdjustTokensBySongCostForFinalStageCheck(false),
	  m_bLoadingNextSong(false),
	  m_pCurSong(MessageID_Invalid),
	  m_pPreferredSong(nullptr),
	  m_pCurSteps(MessageID_Invalid),
	  m_pCurCourse(MessageID_Invalid),
	  m_pCurTrail(MessageID_Invalid),
	  m_bGameplayLeadIn(MessageID_Invalid),
	  m_stEdit(MessageID_Invalid),
	  m_cdEdit(MessageID_Invalid),
	  m_pEditSourceSteps(MessageID_Invalid),
	  m_stEditSource(MessageID_Invalid),
	  m_iEditCourseEntryIndex(MessageID_Invalid),
	  m_sEditLocalProfileID(MessageID_Invalid)
{
	for (auto& s : m_SeparatedStyles) s = nullptr;
	for (bool& joined : m_bSideIsJoined) joined = false;
	for (auto& status : m_MultiPlayerStatus) status = MultiPlayerStatus_NotJoined;
	m_SongOptions.Init();
	for (int& tokens : m_iPlayerStageTokens) tokens = 0;
}
GameState::~GameState() = default;
void GameState::Reset() {}
void GameState::ResetPlayer(PlayerNumber) {}
void GameState::ResetPlayerOptions(PlayerNumber) {}
bool GameState::IsCourseMode() const {
	return m_PlayMode == PLAY_MODE_ONI || m_PlayMode == PLAY_MODE_NONSTOP ||
	       m_PlayMode == PLAY_MODE_ENDLESS;
}
void GameState::GetDefaultPlayerOptions(PlayerOptions& po) {
	po.Init();
	po.FromString(PREFSMAN->m_sDefaultModifiers);
	po.FromString(CommonMetrics::DEFAULT_MODIFIERS);
	if (po.m_sNoteSkin.empty()) po.m_sNoteSkin = CommonMetrics::DEFAULT_NOTESKIN_NAME;
}
void GameState::ApplyCmdline() {}
void GameState::ApplyGameCommand(const RString&, PlayerNumber) {}
void GameState::BeginGame() {}
void GameState::JoinPlayer(PlayerNumber) {}
void GameState::UnjoinPlayer(PlayerNumber) {}
bool GameState::JoinInput(PlayerNumber) { return false; }
bool GameState::JoinPlayers() { return false; }
void GameState::LoadProfiles(bool) {}
void GameState::SavePlayerProfiles() {}
void GameState::SavePlayerProfile(PlayerNumber) {}
bool GameState::HaveProfileToLoad() { return false; }
bool GameState::HaveProfileToSave() { return false; }
void GameState::SaveLocalData() {}
void GameState::AddStageToPlayer(PlayerNumber) {}
void GameState::LoadCurrentSettingsFromProfile(PlayerNumber) {}
void GameState::SaveCurrentSettingsToProfile(PlayerNumber) {}
Song* GameState::GetDefaultSong() const { return nullptr; }
bool GameState::CanSafelyEnterGameplay(RString&) { return true; }
void GameState::SetCompatibleStylesForPlayers() {}
void GameState::ForceSharedSidesMatch() {}
void GameState::ForceOtherPlayersToCompatibleSteps(PlayerNumber) {}
void GameState::Update(float) {}
void GameState::SetCurGame(const Game*) {}
bool GameState::DifficultiesLocked() const { return false; }
bool GameState::ChangePreferredDifficultyAndStepsType(PlayerNumber, Difficulty, StepsType) { return false; }
bool GameState::ChangePreferredDifficulty(PlayerNumber, int) { return false; }
bool GameState::ChangePreferredCourseDifficultyAndStepsType(PlayerNumber, CourseDifficulty, StepsType) { return false; }
bool GameState::ChangePreferredCourseDifficulty(PlayerNumber, int) { return false; }
const Style* GameState::GetCurrentStyle(PlayerNumber) const { return nullptr; }
void GameState::SetProcessedTimingData(TimingData* td) { processedTiming = td; }
TimingData* GameState::GetProcessedTimingData() const { return processedTiming; }
bool GameState::IsCourseDifficultyShown(CourseDifficulty) { return true; }

GameManager::GameManager() = default;
GameManager::~GameManager() = default;
void GameManager::GetStylesForGame(const Game*, std::vector<const Style*>&, bool) {}
const Game* GameManager::GetGameForStyle(const Style*) { return nullptr; }
void GameManager::GetStepsTypesForGame(const Game*, std::vector<StepsType>&) {}
const Style* GameManager::GetEditorStyleForStepsType(StepsType) { return nullptr; }
void GameManager::GetDemonstrationStylesForGame(const Game*, std::vector<const Style*>&) {}
const Style* GameManager::GetHowToPlayStyleForGame(const Game*) { return nullptr; }
void GameManager::GetCompatibleStyles(const Game*, int, std::vector<const Style*>&) {}
const Style* GameManager::GetFirstCompatibleStyle(const Game*, int, StepsType) { return nullptr; }
void GameManager::GetEnabledGames(std::vector<const Game*>&) {}
const Game* GameManager::GetDefaultGame() { return nullptr; }
bool GameManager::IsGameEnabled(const Game*) { return true; }
int GameManager::GetIndexFromGame(const Game*) { return 0; }
const Game* GameManager::GetGameFromIndex(int) { return nullptr; }
const StepsTypeInfo& GameManager::GetStepsTypeInfo(StepsType st) {
	// Provide a complete StepsTypeInfo table so NotesLoaderSM/SSC can correctly
	// parse and preserve steps types (e.g. dance-couple).
	static constexpr int kNumCabinetLightTracks = 6; // matches ITGmania's NUM_CabinetLight
	static const StepsTypeInfo infos[] = {
		// dance
		{ "dance-single", 4, true, StepsTypeCategory_Single },
		{ "dance-double", 8, true, StepsTypeCategory_Double },
		{ "dance-couple", 8, true, StepsTypeCategory_Couple },
		{ "dance-solo", 6, true, StepsTypeCategory_Single },
		{ "dance-threepanel", 3, true, StepsTypeCategory_Single },
		{ "dance-routine", 8, false, StepsTypeCategory_Routine },
		// pump
		{ "pump-single", 5, true, StepsTypeCategory_Single },
		{ "pump-halfdouble", 6, true, StepsTypeCategory_Double },
		{ "pump-double", 10, true, StepsTypeCategory_Double },
		{ "pump-couple", 10, true, StepsTypeCategory_Couple },
		{ "pump-routine", 10, true, StepsTypeCategory_Routine },
		// kb7
		{ "kb7-single", 7, true, StepsTypeCategory_Single },
		// ez2dancer
		{ "ez2-single", 5, true, StepsTypeCategory_Single },
		{ "ez2-double", 10, true, StepsTypeCategory_Double },
		{ "ez2-real", 7, true, StepsTypeCategory_Single },
		// parapara paradise
		{ "para-single", 5, true, StepsTypeCategory_Single },
		// ds3ddx
		{ "ds3ddx-single", 8, true, StepsTypeCategory_Single },
		// beatmania (called "bm" for backward compat)
		{ "bm-single5", 6, true, StepsTypeCategory_Single },
		{ "bm-versus5", 6, true, StepsTypeCategory_Single },
		{ "bm-double5", 12, true, StepsTypeCategory_Double },
		{ "bm-single7", 8, true, StepsTypeCategory_Single },
		{ "bm-versus7", 8, true, StepsTypeCategory_Single },
		{ "bm-double7", 16, true, StepsTypeCategory_Double },
		// dance maniax
		{ "maniax-single", 4, true, StepsTypeCategory_Single },
		{ "maniax-double", 8, true, StepsTypeCategory_Double },
		// technomotion
		{ "techno-single4", 4, true, StepsTypeCategory_Single },
		{ "techno-single5", 5, true, StepsTypeCategory_Single },
		{ "techno-single8", 8, true, StepsTypeCategory_Single },
		{ "techno-double4", 8, true, StepsTypeCategory_Double },
		{ "techno-double5", 10, true, StepsTypeCategory_Double },
		{ "techno-double8", 16, true, StepsTypeCategory_Double },
		// pop'n music (called "pnm" for backward compat)
		{ "pnm-five", 5, true, StepsTypeCategory_Single },
		{ "pnm-nine", 9, true, StepsTypeCategory_Single },
		// cabinet lights
		{ "lights-cabinet", kNumCabinetLightTracks, false, StepsTypeCategory_Single },
		// kickbox mania
		{ "kickbox-human", 4, true, StepsTypeCategory_Single },
		{ "kickbox-quadarm", 4, true, StepsTypeCategory_Single },
		{ "kickbox-insect", 6, true, StepsTypeCategory_Single },
		{ "kickbox-arachnid", 8, true, StepsTypeCategory_Single },
	};
	static_assert(static_cast<int>(sizeof(infos) / sizeof(infos[0])) == static_cast<int>(NUM_StepsType),
	              "StepsTypeInfo table out of sync with StepsType enum");
	static const StepsTypeInfo invalid_info = { "invalid", 0, false, StepsTypeCategory_Single };
	const int sti = static_cast<int>(st);
	if (sti < 0 || sti >= static_cast<int>(NUM_StepsType)) return invalid_info;
	return infos[sti];
}
StepsType GameManager::StringToStepsType(RString s) {
	MakeLower(s);
	Replace(s, '_', '-');
	const int num_steps_types = static_cast<int>(NUM_StepsType);
	for (int i = 0; i < num_steps_types; ++i) {
		if (GetStepsTypeInfo(static_cast<StepsType>(i)).szName == s) return static_cast<StepsType>(i);
	}
	return StepsType_Invalid;
}
const Game* GameManager::StringToGame(RString) { return nullptr; }
const Style* GameManager::GameAndStringToStyle(const Game*, RString) { return nullptr; }
RString GameManager::StyleToLocalizedString(const Style*) { return ""; }
void GameManager::PushSelf(lua_State*) {}

SongManager::SongManager() = default;
SongManager::~SongManager() = default;
Song* SongManager::FindSong(RString) const { return nullptr; }
Song* SongManager::FindSong(RString, RString) const { return nullptr; }

RageTexturePreloader::~RageTexturePreloader() = default;
RageTexturePreloader& RageTexturePreloader::operator=(const RageTexturePreloader&) { return *this; }
void RageTexturePreloader::Load(const RageTextureID&) {}
void RageTexturePreloader::UnloadAll() {}

namespace {
std::vector<IThemeMetric*>& harness_theme_metrics() {
	static std::vector<IThemeMetric*> metrics;
	return metrics;
}

bool harness_font_candidate(
	const std::filesystem::path& fonts_root, const std::string& key,
	std::filesystem::path& result) {
	if (fonts_root.empty()) return false;
	const std::filesystem::path requested = fonts_root / std::filesystem::path(key);
	std::error_code ec;
	if (requested.has_extension() && std::filesystem::is_regular_file(requested, ec)) {
		result = requested;
		return true;
	}

	const std::filesystem::path directory = requested.parent_path();
	const std::string prefix = requested.filename().string();
	std::vector<std::filesystem::path> candidates;
	for (const auto& entry : std::filesystem::directory_iterator(directory, ec)) {
		if (ec) break;
		if (!entry.is_regular_file()) continue;
		const std::string name = entry.path().filename().string();
		if (name.size() < prefix.size() ||
		    CompareNoCase(name.substr(0, prefix.size()), prefix) != 0) {
			continue;
		}
		std::string extension = entry.path().extension().string();
		MakeLower(extension);
		if (extension == ".ini" || extension == ".redir") {
			candidates.push_back(entry.path());
		}
	}
	std::sort(candidates.begin(), candidates.end());
	if (candidates.empty()) return false;
	if (candidates.size() > 1) {
		harness_diagnostics().push_back(
			"multiple font definitions match " + requested.generic_string());
	}
	result = candidates.front();
	return true;
}

bool harness_resolve_font(
	const std::string& key, std::filesystem::path& result, int depth = 0) {
	if (depth >= 100) {
		throw std::runtime_error("font redirect recursion limit exceeded for " + key);
	}
	if (!harness_font_candidate(harness_theme_fonts(), key, result) &&
	    (harness_fallback_fonts() == harness_theme_fonts() ||
	     !harness_font_candidate(harness_fallback_fonts(), key, result))) {
		return false;
	}
	std::string extension = result.extension().string();
	MakeLower(extension);
	if (extension != ".redir") return true;

	std::string redirect;
	if (!GetFileContents(result.generic_string(), redirect, true) || redirect.empty()) {
		throw std::runtime_error("invalid font redirect " + result.generic_string());
	}
	return harness_resolve_font(redirect, result, depth + 1);
}
}
ThemeManager::ThemeManager() : m_sCurThemeName("Harness"), m_sCurLanguage("en"), m_bPseudoLocalize(false) {
	for (IThemeMetric* metric : harness_theme_metrics()) {
		if (metric) metric->Read();
	}
}
ThemeManager::~ThemeManager() = default;
bool ThemeManager::DoesThemeExist(const RString&) { return false; }
void ThemeManager::GetThemeNames(std::vector<RString>&) {}
void ThemeManager::GetSelectableThemeNames(std::vector<RString>&) {}
int ThemeManager::GetNumSelectableThemes() { return 0; }
bool ThemeManager::IsThemeSelectable(RString const&) { return false; }
bool ThemeManager::IsThemeNameValid(RString const&) { return false; }
RString ThemeManager::GetThemeDisplayName(const RString&) { return ""; }
RString ThemeManager::GetThemeAuthor(const RString&) { return ""; }
void ThemeManager::GetLanguages(std::vector<RString>&) {}
bool ThemeManager::DoesLanguageExist(const RString&) { return false; }
void ThemeManager::SwitchThemeAndLanguage(const RString&, const RString&, bool, bool) {}
void ThemeManager::UpdateLuaGlobals() {}
RString ThemeManager::GetNextTheme() { return ""; }
RString ThemeManager::GetNextSelectableTheme() { return ""; }
void ThemeManager::ReloadMetrics() {}
void ThemeManager::ReloadSubscribers() {}
void ThemeManager::ClearSubscribers() {}
void ThemeManager::GetOptionNames(std::vector<RString>&) {}
bool ThemeManager::GetPathInfo(
	PathInfo& out, ElementCategory category, const RString& metrics_group,
	const RString& element, bool) {
	if (category != EC_FONTS) return false;
	const std::string key = metrics_group.empty()
		? element
		: metrics_group + " " + element;
	std::filesystem::path resolved;
	if (!harness_resolve_font(key, resolved)) return false;
	out.sResolvedPath = resolved.generic_string();
	out.sMatchingMetricsGroup = metrics_group;
	out.sMatchingElement = element;
	return true;
}
RString ThemeManager::GetPath(
	ElementCategory category, const RString& metrics_group,
	const RString& element, bool optional) {
	PathInfo info;
	if (GetPathInfo(info, category, metrics_group, element, optional)) {
		return info.sResolvedPath;
	}
	if (!optional) {
		harness_diagnostics().push_back("font theme element not found: " + element);
	}
	return "";
}
void ThemeManager::ClearThemePathCache() {}
bool ThemeManager::HasMetric(const RString&, const RString&) { return false; }
void ThemeManager::Subscribe(IThemeMetric* metric) {
	if (!metric) return;
	auto& metrics = harness_theme_metrics();
	if (std::find(metrics.begin(), metrics.end(), metric) == metrics.end()) {
		metrics.push_back(metric);
	}
	if (THEME && !THEME->GetCurThemeName().empty()) {
		metric->Read();
	}
}
void ThemeManager::Unsubscribe(IThemeMetric* metric) {
	auto& metrics = harness_theme_metrics();
	metrics.erase(std::remove(metrics.begin(), metrics.end(), metric), metrics.end());
}
void ThemeManager::PushMetric(Lua*, const RString&, const RString&) {}
RString ThemeManager::GetMetric(const RString&, const RString&) { return ""; }
int ThemeManager::GetMetricI(const RString&, const RString&) { return 0; }
float ThemeManager::GetMetricF(const RString&, const RString&) { return 0.0f; }
bool ThemeManager::GetMetricB(const RString&, const RString&) { return false; }
RageColor ThemeManager::GetMetricC(const RString&, const RString&) { return RageColor(); }
LuaReference ThemeManager::GetMetricR(const RString&, const RString&) { return LuaReference(); }
void ThemeManager::GetMetric(const RString&, const RString&, LuaReference& out) { out.SetFromNil(); }
bool ThemeManager::HasString(const RString&, const RString&) { return false; }
RString ThemeManager::GetString(const RString&, const RString&) { return ""; }
void ThemeManager::FilterFileLanguages(std::vector<RString>&) {}
void ThemeManager::GetMetricsThatBeginWith(const RString&, const RString&, std::set<RString>&) {}
RString ThemeManager::GetMetricsGroupFallback(const RString&) { return ""; }
RString ThemeManager::GetBlankGraphicPath() { return ""; }
void ThemeManager::RunLuaScripts(const RString&, bool) {}
void ThemeManager::PushSelf(lua_State*) {}

// ---------------------------------------------------------------------------
// Minimal chart structures
#ifndef ITGMANIA_HARNESS_SOURCE
void DisplayBpms::Add(float f) { vfBpms.push_back(f); }
float DisplayBpms::GetMin() const {
	if (vfBpms.empty()) return 0.0f;
	float out = vfBpms.front();
	for (float v : vfBpms) if (v < out) out = v;
	return out;
}
float DisplayBpms::GetMax() const {
	if (vfBpms.empty()) return 0.0f;
	float out = vfBpms.front();
	for (float v : vfBpms) if (v > out) out = v;
	return out;
}
float DisplayBpms::GetMaxWithin(float highest) const {
	float out = GetMax();
	return out > highest ? highest : out;
}
bool DisplayBpms::BpmIsConstant() const { return vfBpms.size() <= 1 || GetMin() == GetMax(); }
bool DisplayBpms::IsSecret() const { return false; }

TimingData::TimingData(float fOffset)
	: m_fBeat0OffsetInSeconds(fOffset),
	  m_fBeat0GroupOffsetInSeconds(0.0f) {}

TimingData::~TimingData() {
	for (auto& vec : m_avpTimingSegments) {
		for (auto* seg : vec) delete seg;
		vec.clear();
	}
}

template<> NoteData* HiddenPtrTraits<NoteData>::Copy(const NoteData*) { return nullptr; }
template<> void HiddenPtrTraits<NoteData>::Delete(NoteData*) {}

Steps::Steps(Song* song)
	: m_Timing(0.0f),
	  m_StepsType(StepsType_Invalid),
	  m_StepsTypeStr(""),
	  m_pSong(song),
	  parent(nullptr),
	  m_bNoteDataIsFilled(false),
	  m_bSavedToDisk(false),
	  m_LoadedFromProfile(ProfileSlot_Invalid),
	  m_iHash(0),
	  m_Difficulty(Difficulty_Invalid),
	  m_iMeter(0),
	  m_bAreCachedRadarValuesJustLoaded(false),
	  m_bAreCachedTechCountsValuesJustLoaded(false),
	  m_AreCachedNpsPerMeasureJustLoaded(false),
	  m_AreCachedNotesPerMeasureJustLoaded(false),
	  m_bIsCachedGrooveStatsHashJustLoaded(false),
	  m_iGrooveStatsHashVersion(0),
	  displayBPMType(DISPLAY_BPM_ACTUAL),
	  specifiedBPMMin(0.0f),
	  specifiedBPMMax(0.0f)
{
	for (auto& rv : m_CachedRadarValues) rv.Zero();
	for (auto& tc : m_CachedTechCounts) tc.Zero();
}

Steps::~Steps() = default;
#endif

// ---------------------------------------------------------------------------
// Song stubs sufficient for parsing
Song::Song()
		: m_SelectionDisplay(SHOW_ALWAYS),
		  m_fMusicSampleStartSeconds(0.0f),
		  m_fMusicSampleLengthSeconds(0.0f),
		  m_DisplayBPMType(DISPLAY_BPM_ACTUAL),
		  m_fSpecifiedBPMMin(0.0f),
		  m_fSpecifiedBPMMax(0.0f),
		  firstSecond(-1.0f),
		  lastSecond(-1.0f),
		  specifiedLastSecond(-1.0f)
{
		for (auto& changes : m_BackgroundChanges) {
			changes = AutoPtrCopyOnWrite<VBackgroundChange>(new VBackgroundChange);
		}
		m_ForegroundChanges = AutoPtrCopyOnWrite<VBackgroundChange>(new VBackgroundChange);
		for (auto& vec : m_vpStepsByType) vec.clear();
}
Song::~Song() { DetachSteps(); }
void Song::Reset() { DetachSteps(); }
void Song::DetachSteps() {
	for (auto* steps : m_vpSteps) delete steps;
	m_vpSteps.clear();
	for (auto& vec : m_vpStepsByType) vec.clear();
}
bool Song::LoadFromSongDir(RString, bool, ProfileSlot) { return false; }
bool Song::ReloadFromSongDir(RString) { return false; }
void Song::LoadEditsFromSongDir(RString) {}
bool Song::HasAutosaveFile() { return false; }
bool Song::LoadAutosaveFile() { return false; }
void Song::TidyUpData(bool, bool) {}
void Song::ReCalculateStepStatsAndLastSecond(bool, bool) {}
void Song::TranslateTitles() {}
void Song::AddBackgroundChange(BackgroundLayer layer, BackgroundChange seg) {
	BackgroundUtil::AddBackgroundChange(GetBackgroundChanges(layer), seg);
}
void Song::AddForegroundChange(BackgroundChange seg) {
	BackgroundUtil::AddBackgroundChange(GetForegroundChanges(), seg);
}
bool Song::HasSignificantBpmChangesOrStops() const { return false; }
void Song::GetDisplayBpms(DisplayBpms& bpms) const { bpms.Add(m_fSpecifiedBPMMin); bpms.Add(m_fSpecifiedBPMMax); }
RString Song::GetDisplayMainTitle() const { return m_sMainTitleTranslit.empty() ? m_sMainTitle : m_sMainTitleTranslit; }
RString Song::GetDisplaySubTitle() const { return m_sSubTitleTranslit.empty() ? m_sSubTitle : m_sSubTitleTranslit; }
RString Song::GetDisplayArtist() const { return m_sArtistTranslit.empty() ? m_sArtist : m_sArtistTranslit; }
RString Song::GetMainTitle() const { return m_sMainTitle; }
RString Song::GetSongAssetPath(RString sPath, const RString& sSongPath) {
	if (sPath.empty()) return sPath;
	if (std::filesystem::path(sPath.c_str()).is_absolute()) return sPath;
	return (std::filesystem::path(sSongPath.c_str()) / sPath.c_str()).string().c_str();
}
Steps* Song::CreateSteps() { return new Steps(this); }
void Song::AddSteps(Steps* steps) {
	if (!steps) return;
	m_vpSteps.push_back(steps);
	if (steps->m_StepsType >= 0 && steps->m_StepsType < NUM_StepsType) {
		m_vpStepsByType[steps->m_StepsType].push_back(steps);
	}
}
const std::vector<BackgroundChange>& Song::GetBackgroundChanges(BackgroundLayer bl) const {
	return *m_BackgroundChanges[bl];
}
std::vector<BackgroundChange>& Song::GetBackgroundChanges(BackgroundLayer bl) {
	return *m_BackgroundChanges[bl].Get();
}
const std::vector<BackgroundChange>& Song::GetForegroundChanges() const { return *m_ForegroundChanges; }
std::vector<BackgroundChange>& Song::GetForegroundChanges() { return *m_ForegroundChanges.Get(); }
float Song::GetFirstSecond() const { return firstSecond; }
float Song::GetLastBeat() const { return m_SongTiming.GetBeatFromElapsedTime(lastSecond); }
float Song::GetLastSecond() const { return lastSecond; }
float Song::GetSpecifiedLastBeat() const { return m_SongTiming.GetBeatFromElapsedTime(specifiedLastSecond); }
float Song::GetSpecifiedLastSecond() const { return specifiedLastSecond; }
// Song.cpp's path accessor, using the asset path supplied by the native loader.
RString Song::GetBackgroundPath() const { return GetSongAssetPath(m_sBackgroundFile, m_sSongDir); }
void Song::SetSpecifiedLastSecond(const float f) { specifiedLastSecond = f; }
void Song::SetFirstSecond(const float f) { firstSecond = f; }
void Song::SetLastSecond(const float f) { lastSecond = f; }
int Song::GetNumStepsLoadedFromProfile(ProfileSlot) const { return 0; }
bool Song::IsEditAlreadyLoaded(Steps*) const { return false; }
std::string Song::GetTranslitFullTitle() const {
	std::string title = GetTranslitMainTitle();
	std::string subtitle = GetTranslitSubTitle();
	if (!subtitle.empty()) title += " " + subtitle;
	return title;
}
std::vector<std::string> Song::GetInstrumentTracksToVectorString() const {
	std::vector<std::string> out;
	FOREACH_ENUM(InstrumentTrack, it) {
		if (!m_sInstrumentTrackFile[it].empty()) {
			out.push_back(InstrumentTrackToString(it) + "=" + m_sInstrumentTrackFile[it]);
		}
	}
	return out;
}

// ---------------------------------------------------------------------------
// ScreenMessage helpers
#ifndef ITGMANIA_HARNESS_SOURCE
ScreenMessage ScreenMessageHelpers::ToScreenMessage(const RString& name) { return name; }
RString ScreenMessageHelpers::ScreenMessageToString(ScreenMessage sm) { return sm; }
#endif

const std::vector<RString>& ActorUtil::GetTypeExtensionList(FileType) {
	static std::vector<RString> empty;
	return empty;
}

const RString RANDOM_BACKGROUND_FILE = "";
const RString NO_SONG_BG_FILE = "";
const RString SONG_BACKGROUND_FILE = "";
const RString SBE_UpperLeft = "";
const RString SBE_Centered = "";
const RString SBE_StretchNormal = "";
const RString SBE_StretchNoLoop = "";
const RString SBE_StretchRewind = "";
const RString SBT_CrossFade = "";
const RString EDIT_STEPS_SUBDIR = "Edits/";
const RString EDIT_COURSES_SUBDIR = "EditCourses/";

#ifndef ITGMANIA_HARNESS_SOURCE
void XNodeStringValue::GetValue(RString& out) const { out = m_sValue; }
void XNodeStringValue::GetValue(int& out) const {
	std::stringstream ss;
	ss << m_sValue;
	ss >> out;
	if (ss.fail()) out = 0;
}
void XNodeStringValue::GetValue(float& out) const {
	std::stringstream ss;
	ss << m_sValue;
	ss >> out;
	if (ss.fail()) out = 0.0f;
}
void XNodeStringValue::GetValue(bool& out) const {
	RString lower = m_sValue;
	MakeLower(lower);
	out = (lower == "1" || lower == "true" || lower == "yes" || lower == "on");
}
void XNodeStringValue::GetValue(unsigned& out) const {
	std::stringstream ss;
	ss << m_sValue;
	unsigned long tmp = 0;
	ss >> tmp;
	out = ss.fail() ? 0u : static_cast<unsigned>(tmp);
}
void XNodeStringValue::PushValue(lua_State* L) const { lua_pushstring(L, m_sValue.c_str()); }
void XNodeStringValue::SetValue(const RString& v) { m_sValue = v; }
void XNodeStringValue::SetValue(int v) { m_sValue = std::to_string(v).c_str(); }
void XNodeStringValue::SetValue(float v) {
	std::ostringstream oss;
	oss << v;
	m_sValue = oss.str().c_str();
}
void XNodeStringValue::SetValue(unsigned v) { m_sValue = std::to_string(v).c_str(); }
void XNodeStringValue::SetValueFromStack(lua_State* L) {
	if (!L) return;
	const char* s = lua_tostring(L, -1);
	m_sValue = s ? s : "";
}
#endif

namespace BackgroundUtil {
void AddBackgroundChange(std::vector<BackgroundChange>& changes, BackgroundChange change) {
	changes.push_back(std::move(change));
	SortBackgroundChangesArray(changes);
}
void SortBackgroundChangesArray(std::vector<BackgroundChange>& changes) {
	std::sort(changes.begin(), changes.end(), [](const BackgroundChange& a, const BackgroundChange& b) {
		return a.m_fStartBeat < b.m_fStartBeat;
	});
}
void GetBackgroundEffects(const RString&, std::vector<RString>& paths, std::vector<RString>& names) { paths.clear(); names.clear(); }
void GetBackgroundTransitions(const RString&, std::vector<RString>& paths, std::vector<RString>& names) { paths.clear(); names.clear(); }
void GetSongBGAnimations(const Song*, const RString&, std::vector<RString>& paths, std::vector<RString>& names) { paths.clear(); names.clear(); }
void GetSongMovies(const Song*, const RString&, std::vector<RString>& paths, std::vector<RString>& names) { paths.clear(); names.clear(); }
void GetSongBitmaps(const Song*, const RString&, std::vector<RString>& paths, std::vector<RString>& names) { paths.clear(); names.clear(); }
void GetGlobalBGAnimations(const Song*, const RString&, std::vector<RString>& paths, std::vector<RString>& names) { paths.clear(); names.clear(); }
void GetGlobalRandomMovies(const Song*, const RString&, std::vector<RString>& paths, std::vector<RString>& names, bool, bool) { paths.clear(); names.clear(); }
void BakeAllBackgroundChanges(Song*) {}
}

std::string BackgroundChange::ToString() const {
	return ssprintf(
		"%.3f=%s=%.3f=%d=%d=%d=%s=%s=%s=%s=%s",
		m_fStartBeat,
		SmEscape(m_def.m_sFile1).c_str(),
		m_fRate,
		m_sTransition == SBT_CrossFade,
		m_def.m_sEffect == SBE_StretchRewind,
		m_def.m_sEffect != SBE_StretchNoLoop,
		m_def.m_sEffect.c_str(),
		m_def.m_sFile2.c_str(),
		m_sTransition.c_str(),
		SmEscape(RageColor::NormalizeColorString(m_def.m_sColor1)).c_str(),
		SmEscape(RageColor::NormalizeColorString(m_def.m_sColor2)).c_str());
}

const std::string& ProfileManager::GetProfileDir(ProfileSlot) const {
	static const std::string empty;
	return empty;
}

#ifndef ITGMANIA_HARNESS_SOURCE
XNode::XNode() : m_sName("") {}
XNode::XNode(const RString& sName) : m_sName(sName) {}
XNode::XNode(const XNode& cpy) : m_sName(cpy.m_sName) {}
const XNodeValue* XNode::GetAttr(const RString& sAttrName) const {
	auto it = m_attrs.find(sAttrName);
	return it != m_attrs.end() ? it->second : nullptr;
}
XNodeValue* XNode::GetAttr(const RString& sAttrName) {
	auto it = m_attrs.find(sAttrName);
	return it != m_attrs.end() ? it->second : nullptr;
}
bool XNode::PushAttrValue(lua_State*, const RString&) const { return false; }
const XNode* XNode::GetChild(const RString& sName) const {
	for (auto* child : m_childs) {
		if (child && child->GetName() == sName) return child;
	}
	return nullptr;
}
XNode* XNode::GetChild(const RString& sName) { return const_cast<XNode*>(static_cast<const XNode*>(this)->GetChild(sName)); }
bool XNode::PushChildValue(lua_State*, const RString&) const { return false; }
XNode* XNode::AppendChild(XNode* node) {
	if (!node) return nullptr;
	m_childs.push_back(node);
	m_children_by_name.emplace(node->GetName(), node);
	return node;
}
bool XNode::RemoveChild(XNode* node, bool bDelete) {
	if (!node) return false;
	for (auto it = m_childs.begin(); it != m_childs.end(); ++it) {
		if (*it == node) {
			m_children_by_name.erase(node->GetName());
			if (bDelete) delete node;
			m_childs.erase(it);
			return true;
		}
	}
	return false;
}
void XNode::RemoveChildFromByName(XNode* node) {
	if (!node) return;
	m_children_by_name.erase(node->GetName());
}
void XNode::RenameChildInByName(XNode* node) {
	if (!node) return;
	RemoveChildFromByName(node);
	m_children_by_name.emplace(node->GetName(), node);
}
XNodeValue* XNode::AppendAttrFrom(const RString& sName, XNodeValue* value, bool bOverwrite) {
	if (!value) return nullptr;
	auto it = m_attrs.find(sName);
	if (it != m_attrs.end()) {
		if (bOverwrite) {
			delete it->second;
			it->second = value;
		}
		return it->second;
	}
	m_attrs[sName] = value;
	return value;
}
XNodeValue* XNode::AppendAttr(const RString& sName) { return AppendAttrFrom(sName, new XNodeStringValue, true); }
bool XNode::RemoveAttr(const RString& sName) {
	auto it = m_attrs.find(sName);
	if (it == m_attrs.end()) return false;
	delete it->second;
	m_attrs.erase(it);
	return true;
}
void XNode::Clear() { Free(); }
void XNode::Free() {
	for (auto* child : m_childs) delete child;
	m_childs.clear();
	m_children_by_name.clear();
	for (auto& kv : m_attrs) delete kv.second;
	m_attrs.clear();
}
#endif

namespace DWILoader {
void GetApplicableFiles(const RString&, std::vector<RString>& out) { out.clear(); }
bool LoadFromDir(const RString&, Song&, std::set<RString>&) { return false; }
bool LoadNoteDataFromSimfile(const RString&, Steps&) { return false; }
}
namespace KSFLoader {
void GetApplicableFiles(const RString&, std::vector<RString>& out) { out.clear(); }
bool LoadFromDir(const RString&, Song&) { return false; }
bool LoadNoteDataFromSimfile(const RString&, Steps&) { return false; }
}
namespace BMSLoader {
void GetApplicableFiles(const RString&, std::vector<RString>& out) { out.clear(); }
bool LoadFromDir(const RString&, Song&) { return false; }
bool LoadNoteDataFromSimfile(const RString&, Steps&) { return false; }
}

#ifndef ITGMANIA_HARNESS_SOURCE
namespace StringConversion {
template<> bool FromString<float>(const RString& sValue, float& out) {
	std::stringstream ss;
	ss << sValue;
	ss >> out;
	return !ss.fail() && ss.eof();
}
template<> bool FromString<bool>(const RString& sValue, bool& out) {
	RString lower = sValue;
	MakeLower(lower);
	if (lower == "1" || lower == "true" || lower == "yes" || lower == "on") { out = true; return true; }
	if (lower == "0" || lower == "false" || lower == "no" || lower == "off") { out = false; return true; }
	return false;
}
template<> RString ToString<float>(const float& value) {
	std::ostringstream oss;
	oss << value;
	return oss.str().c_str();
}
template<> RString ToString<bool>(const bool& value) { return value ? "true" : "false"; }
} // namespace StringConversion
#endif

// ---------------------------------------------------------------------------
// Simple initialization helper to create globals
static void init_globals_once() {
	static bool done = false;
	if (done) return;

	// Construct only the few preferences we need by hand.
    static std::aligned_storage_t<sizeof(PrefsManager), alignof(PrefsManager)> prefs_storage{};
    PrefsManager* prefs_raw = reinterpret_cast<PrefsManager*>(&prefs_storage);
	new (&prefs_raw->m_fGlobalOffsetSeconds) Preference<float>("GlobalOffsetSeconds", 0.0f);
	new (&prefs_raw->m_bQuirksMode) Preference<bool>("QuirksMode", false);
	new (&prefs_raw->m_bLightsSimplifyBass) Preference<bool>("LightsSimplifyBass", false);
	new (&prefs_raw->m_MinTNSToHideNotes) Preference<TapNoteScore>("MinTNSToHideNotes", TNS_W3);
	PREFSMAN = prefs_raw;

	done = true;
}

struct GlobalInit {
	GlobalInit() { init_globals_once(); }
} _globalInit;

#ifndef ITGMANIA_HARNESS_SOURCE
void init_itgmania_runtime(int, char**) { init_globals_once(); }
#endif

#if !defined(ITGMANIA_BUNDLED_LUA)
int luaL_pushtype(lua_State* L, int n) {
	const char* t = lua_typename(L, lua_type(L, n));
	lua_pushstring(L, t ? t : "");
	return 1;
}
#endif
