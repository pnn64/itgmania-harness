"""Check screen replacement using a complete native ITGmania install."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import wave


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--itgmania-bin", type=Path, required=True)
    parser.add_argument("--harness-bin", type=Path, required=True)
    parser.add_argument("--fallback-theme", required=True)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--until-gameplay-end", action="store_true")
    parser.add_argument("--autoplay", choices=("0", "1"), default="1")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    fixture = root / "tests/fixtures/song-lua-runtime-reload"
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
                   str(song / "reload.ssc"), "--itgmania-bin",
                   str(args.itgmania_bin.resolve()), "--fallback-theme",
                   args.fallback_theme, "--difficulty", "Challenge",
                   "--headless", "--autoplay", args.autoplay, "--timeout", "45",
                   "--out", str(trace)]
        command += ["--until-gameplay-end"] if args.until_gameplay_end else ["--until-beat", "2"]
        subprocess.run(command, cwd=root, check=True, timeout=60)
        document = json.loads(trace.read_bytes())
        samples = [event["args"][1] for event in document["events"]
                   if event["operation"] == "MessageManager.Broadcast"
                   and event["args"][0] == "HarnessReloadControl"]
        assert document["display"]["video_renderers"] == "null", document["display"]
        assert document["dropped_events"] == 0, document["dropped_events"]
        assert not [event for event in document["events"] if event["kind"] == "error"]
        assert [sample["count"] for sample in samples] == [1, 2], samples
        assert [sample["difficulty"] for sample in samples] == [
            "Difficulty_Challenge", "Difficulty_Easy"], samples
        assert all(sample["top_name"] == "ScreenGameplay" for sample in samples), samples
        assert all(sample["current_possible"] == 0 for sample in samples), samples
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
        if args.out:
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_bytes(trace.read_bytes())
        with args.itgmania_bin.open("rb") as executable:
            native_hash = hashlib.file_digest(executable, "sha256").hexdigest()
        print(json.dumps({"native_exe_sha256": native_hash,
                          "samples": samples, "events": len(document["events"]),
                          "completion": document.get("completion"),
                          "final_stage": stages[-1]}))


if __name__ == "__main__":
    main()
