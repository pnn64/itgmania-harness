// Compile the entire native scorekeeper. Manager access is checked at the
// application boundary so unsupported toasties cannot dereference a null
// singleton or certify a healthy capture. The scoring implementation is intact.
#include "ScreenManager.h"
#include "ProfileManager.h"
#include "runtime_stubs.h"
#define SCREENMAN harness_score_screen()
#define PROFILEMAN harness_score_profile()
#include "ScoreKeeperNormal.cpp"
