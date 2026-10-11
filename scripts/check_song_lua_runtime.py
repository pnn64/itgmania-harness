"""Check song actor lifecycles using a complete native ITGmania install."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
import subprocess
import tempfile
import wave


def check_foregrounds(document):
    events = [event for event in document["events"]
              if event["operation"] == "MessageManager.Broadcast"
              and event["args"][0].startswith("HarnessForeground")]
    constructs = [event for event in events if event["args"][0] == "HarnessForegroundConstruct"]
    starts = [event for event in events if event["args"][0] == "HarnessForegroundOn"]
    later = [event for event in events if event["args"][0] == "HarnessForegroundLater"]
    assert [event["args"][1]["label"] for event in constructs] == ["early", "late"], constructs
    assert [event["args"][1]["label"] for event in starts] == ["early", "late"], starts
    assert [event["args"][1]["label"] for event in later] == ["early", "late"], later
    assert constructs[-1]["seq"] < starts[0]["seq"], events
    assert 2 <= starts[0]["beat"] < 2.02 and 4 <= starts[1]["beat"] < 4.02, starts
    for label in ("early", "late"):
        updates = [event for event in events if event["args"][0] == "HarnessForegroundUpdate"
                   and event["args"][1]["label"] == label]
        assert len(updates) == 2, updates
        assert [event["args"][1]["count"] for event in updates] == [1, 2], updates
        # Foreground::Update starts with music_seconds - start_second. At this
        # fixture's constant 120 BPM that is the beat overshoot divided by two.
        start_beat = 2 if label == "early" else 4
        assert math.isclose(updates[0]["args"][1]["delta"],
                            (updates[0]["beat"] - start_beat) / 2, abs_tol=1e-7), updates
        assert updates[1]["args"][1]["delta"] > 0, updates
        assert math.isclose(updates[1]["args"][1]["delta"],
                            updates[1]["seconds"] - updates[0]["seconds"], abs_tol=1e-7), updates
    assert document["completion"]["reason"] == "gameplay-exit", document["completion"]
    assert document["completion"]["stage_committed"], document["completion"]
    assert any(event["operation"].endswith("StartTransitioningScreen") for event in document["events"])


def check_hold_row(document):
    events = [event for event in document["events"]
              if event["operation"] == "MessageManager.Broadcast"
              and event["args"][0] == "HarnessHoldRow"]
    assert [event["args"][1]["current_possible"] for event in events] == [0, 5, 10], events
    # Player::UpdateHoldNotes scores at BeatToNoteRow(song_beat) >= tail_row,
    # rounded on the fixed 48-row beat grid, before this hold's exact tail beat.
    tail = events[-1]
    assert 1 - 0.5 / 48 <= tail["beat"] < 1, tail
    assert document["completion"]["reason"] == "gameplay-exit", document["completion"]
    assert document["completion"]["stage_committed"], document["completion"]
    final = document["stage_samples"][-1]["players"][0]
    assert final["possible"] == final["current_possible"] == 10, final
    assert final["misses"] == 1, final


def check_reload_queue(document):
    events = [event for event in document["events"]
              if event["operation"] == "MessageManager.Broadcast"
              and event["args"][0].startswith("HarnessReloadQueue")]
    assert [event["args"][0] for event in events] == [
        "HarnessReloadQueueOn", "HarnessReloadQueueOn",
        "HarnessReloadQueueTick", "HarnessReloadQueueDone"], events
    assert [event["args"][1]["count"] for event in events] == [1, 2, 2, 2], events
    tick, done = events[-2:]
    assert 0.05 <= tick["seconds"] < 0.08, tick
    # Queue execution carries the first timer's residual frame time into the
    # next sleep, so callback timestamps can differ by less than 1.3 seconds.
    assert 1.27 <= done["seconds"] - tick["seconds"] < 1.33, events
    assert tick["args"][1]["old_updates"] > 0, tick
    assert tick["args"][1]["old_updates"] == done["args"][1]["old_updates"], events
    assert document["completion"]["reason"] == "gameplay-exit", document["completion"]
    assert document["completion"]["stage_committed"], document["completion"]
    requests = [event for event in document["events"]
                if event["operation"] == "ScreenManager.SetNewScreen"]
    assert len(requests) == 1 and requests[0]["args"] == ["ScreenGameplay"], requests
    transitions = [event for event in document["events"]
                   if event["operation"].endswith("StartTransitioningScreen")]
    assert len(transitions) == 1 and transitions[0]["args"] == ["SM_DoNextScreen"], transitions


def check_broadcast_params(document):
    events = [event for event in document["events"]
              if event["operation"] == "MessageManager.Broadcast"
              and event["args"][0].startswith("HarnessParams")]
    assert [event["args"][0] for event in events] == [
        "HarnessParamsSnapshot", "HarnessParamsObserved", "HarnessParamsEmpty"], events
    assert events[0]["args"][1] == {"count": 2, "nested": [True, "original"], "3": "mixed"}, events
    # MessageManager.cpp retains the parameter table and dispatches it. It
    # never probes arbitrary data for GetName or GetParent actor methods.
    assert events[1]["args"][1] == {"lookups": 0, "count": 99, "text": "mutated"}, events
    assert events[2]["args"][1] == [], events
    assert document["completion"]["reason"] == "gameplay-exit", document["completion"]
    assert document["completion"]["stage_committed"], document["completion"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--itgmania-bin", type=Path, required=True)
    parser.add_argument("--harness-bin", type=Path, required=True)
    parser.add_argument("--fallback-theme", required=True)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--until-gameplay-end", action="store_true")
    parser.add_argument("--autoplay", choices=("0", "1"), default="1")
    parser.add_argument("--fixture", choices=("reload", "reload-queue", "foreground", "hold-row", "broadcast-params"), default="reload")
    parser.add_argument("--keep-run-dir", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    fixture = root / f"tests/fixtures/song-lua-runtime-{args.fixture}"
    with tempfile.TemporaryDirectory(prefix="native-reload-control-") as temporary:
        song = Path(temporary) / "song"
        shutil.copytree(fixture, song)
        with wave.open(str(song / "music.wav"), "wb") as music:
            music.setnchannels(1)
            music.setsampwidth(2)
            music.setframerate(44100)
            music.writeframes(b"\0\0" * 44100 * 8)
        trace = Path(temporary) / "trace.json"
        command = [str(args.harness_bin.resolve()), "song-lua-trace",
                   str(song / f"{args.fixture}.ssc"), "--itgmania-bin",
                   str(args.itgmania_bin.resolve()), "--fallback-theme",
                   args.fallback_theme, "--difficulty", "Challenge",
                   "--headless", "--autoplay", args.autoplay, "--timeout", "45",
                   "--out", str(trace)]
        command += ["--until-gameplay-end"] if args.until_gameplay_end else ["--until-beat", "2"]
        if args.keep_run_dir:
            command += ["--keep-run-dir"]
        subprocess.run(command, cwd=root, check=True, timeout=60)
        document = json.loads(trace.read_bytes())
        # Keep the requested diagnostic trace even if validation fails. The
        # nonzero exit status still prevents treating it as an accepted control.
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_bytes(trace.read_bytes())
        samples = [event["args"][1] for event in document["events"]
                   if event["operation"] == "MessageManager.Broadcast"
                   and event["args"][0] == "HarnessReloadControl"]
        assert document["display"]["video_renderers"] == "null", document["display"]
        assert document["dropped_events"] == 0, document["dropped_events"]
        frames = document["update_frames"]
        assert frames and document["capabilities"]["native_update_frames"]
        assert document["capabilities"]["native_machine_preferences"]
        preferences = document["input"]["preferences"]
        assert preferences["default_sync_offset"] in ("SyncOffset_NULL", "SyncOffset_ITG")
        assert isinstance(preferences["global_offset_seconds"], (int, float))
        assert isinstance(preferences["visual_delay_seconds"], (int, float))
        assert document["capabilities"]["native_gameplay_preferences"]
        gameplay = document["input"]["gameplay_preferences"]
        assert gameplay["mod_level"] == "ModsLevel_Preferred", gameplay
        if "Simply Love" in args.fallback_theme or "Simply-Love" in args.fallback_theme:
            # SL_Init.lua's ITG preferences, applied by the real theme helper.
            assert gameplay["initialization"] == "theme-game-mode", gameplay
            assert gameplay["game_mode"] == "ITG", gameplay
            windows = {"W1": 0.0215, "W2": 0.043, "W3": 0.102, "W4": 0.135,
                       "W5": 0.18, "Hold": 0.32, "Mine": 0.07, "Roll": 0.35}
            assert all(abs(gameplay["timing_windows"][key] - value) < 1e-8
                       for key, value in windows.items()), gameplay
            assert abs(gameplay["timing_window_add"] - 0.0015) < 1e-8, gameplay
            assert gameplay["timing_window_scale"] == gameplay["music_rate"] == 1, gameplay
            assert all(player["min_tns_to_hide_notes"] == "TapNoteScore_W3"
                       and player["disabled_timing_windows"] == []
                       for player in gameplay["players"]), gameplay
        assert document["dropped_update_frames"] == 0
        assert document["update_frame_phase"] == "gameplay-overlay-probe"
        assert [frame["sequence"] for frame in frames] == list(range(1, len(frames) + 1))
        assert {frame["scene"] for frame in frames} == (
            {1, 2} if args.fixture in ("reload", "reload-queue") else {1})
        assert all(frame["delta"] >= 0 for frame in frames)
        assert all(right["wall_seconds"] >= left["wall_seconds"] for left, right in zip(frames, frames[1:]))
        assert not [event for event in document["events"] if event["kind"] == "error"]
        if args.fixture != "reload":
            {"foreground": check_foregrounds, "hold-row": check_hold_row,
             "reload-queue": check_reload_queue,
             "broadcast-params": check_broadcast_params}[args.fixture](document)
            print(json.dumps({"fixture": args.fixture, "events": len(document["events"]),
                              "completion": document["completion"], "update_frames": len(frames)}))
            return
        assert [sample["count"] for sample in samples] == [1, 2], samples
        assert [sample["difficulty"] for sample in samples] == [
            "Difficulty_Challenge", "Difficulty_Easy"], samples
        assert all(sample["top_name"] == "ScreenGameplay" for sample in samples), samples
        assert all(sample["current_possible"] == 0 for sample in samples), samples
        queries = [event for event in document["events"]
                   if event["operation"] in ("PlayerStageStats.GetCurrentPossibleDancePoints",
                                             "PlayerStageStats.GetPossibleDancePoints")]
        assert len(queries) == 4, queries
        assert all(event.get("actor", "").startswith("stage:current/player:")
                   for event in queries), queries
        assert samples[1]["possible"] > 0, samples
        assert samples[0]["possible"] == 2 * samples[1]["possible"], samples
        stages = document["stage_samples"]
        assert {sample["scene"] for sample in stages} == {1, 2}, stages
        if args.until_gameplay_end:
            assert document["completion"]["reason"] == "gameplay-exit", document["completion"]
            assert document["completion"]["stage_committed"], document["completion"]
            final = stages[-1]["players"][0]
            assert final["autoplay"] == int(args.autoplay), final
            if args.autoplay == "1":
                # Player::HandleTapRowScore returns before scoring for PC_AUTOPLAY.
                assert final["current_possible"] == final["actual"] == 0, final
                assert final["w1"] == final["w2"] == final["misses"] == 0, final
            else:
                assert final["current_possible"] == final["possible"], final
                assert final["misses"] == 2, final
                assert final["actual"] == 2 * document["scoring_weights"]["Miss"], final
                assert final["life_latest"] < final["life_begin"], final
            assert any(sample["phase"] == "screen-end" for sample in stages), stages
        with args.itgmania_bin.open("rb") as executable:
            native_hash = hashlib.file_digest(executable, "sha256").hexdigest()
        print(json.dumps({"native_exe_sha256": native_hash,
                          "samples": samples, "events": len(document["events"]),
                          "completion": document.get("completion"),
                          "final_stage": stages[-1]}))


if __name__ == "__main__":
    main()
