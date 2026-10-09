use std::path::PathBuf;
use std::process::Command;

fn run_fixture(name: &str) -> serde_json::Value {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/actors")
        .join(name);
    let output = Command::new(env!("CARGO_BIN_EXE_itgmania-harness-rs"))
        .args(["actor-conformance", fixture.to_str().unwrap()])
        .output()
        .expect("actor conformance process should start");
    assert!(
        output.status.success(),
        "actor fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("actor fixture output should be JSON")
}

#[test]
fn captures_native_tween_queue_and_projected_geometry() {
    let output = run_fixture("tween-queue.json");
    assert_eq!(output["oracle"], "itgmania_native_actor_conformance");
    assert_eq!(
        output["samples"][0]["actors"][1]["tween_queue"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        output["samples"][0]["actors"][1]["draws"][0]["vertices"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(output["samples"][3]["actors"][1]["world_matrix"].is_array());
}

#[test]
fn model_uvs_honor_native_vertex_texture_flags() {
    let output = run_fixture("model-texture-scale.json");
    for sample in output["samples"].as_array().unwrap() {
        let draws = sample["actors"][1]["draws"].as_array().unwrap();
        assert_eq!(draws.len(), 2, "diffuse and glow draws");
        for draw in draws {
            assert_eq!(
                draw["texture_matrix_scale"],
                serde_json::json!([[1.0, 1.0], [0.0, 1.0], [1.0, 0.0]])
            );
            // Texture matrix scaling.vert blends transformed and raw UVs.
            // Vertex flags disable each axis independently, including glow.
            for (vertex, expected) in draw["vertices"].as_array().unwrap().iter().zip([
                [0.125, 0.25],
                [1.0, 0.25],
                [0.125, 1.0],
            ]) {
                assert_eq!(vertex["transformed_uv"], serde_json::json!(expected));
            }
        }
    }
}

#[test]
fn model_geometry_uses_the_native_hardware_mesh_path() {
    let output = run_fixture("model-merged-meshes.json");
    let draws = output["samples"][0]["actors"][1]["draws"]
        .as_array()
        .unwrap();
    assert_eq!(
        draws.len(),
        4,
        "two diffuse meshes followed by two glow meshes"
    );
    for (draw, (mesh, vertices, mode)) in draws.iter().zip([
        (0, 6, "modulate"),
        (1, 3, "modulate"),
        (0, 6, "glow"),
        (1, 3, "glow"),
    ]) {
        // RageModelGeometry::MergeMeshes appends mesh 1 to mesh 0 but retains
        // mesh 1. Preserve that native draw behavior, including duplicate faces.
        assert_eq!(draw["model_mesh_index"], mesh);
        assert_eq!(draw["model_mesh_name"], "joined mesh");
        assert_eq!(draw["vertices"].as_array().unwrap().len(), vertices);
        assert_eq!(draw["texture_mode"], mode);
    }
    assert_eq!(
        draws[0]["vertices"][3]["local"],
        draws[1]["vertices"][0]["local"]
    );
    assert_eq!(
        draws[0]["vertices"][3]["world"],
        draws[1]["vertices"][0]["world"]
    );
}

#[test]
fn manual_player_poses_match_the_checked_in_native_golden() {
    let output = run_fixture("manual-player-draws.json");
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/manual-player-native.json"))
            .expect("checked-in native manual-draw baseline");
    // The original provenance identifies the old workspace commit. Compare the
    // captured geometry directly; upstream source revisions are pinned by Git.
    let sprites = golden["sprites"].as_object().expect("native sprite map");
    let actors = output["samples"][0]["actors"]
        .as_array()
        .expect("native actors");
    assert_eq!(sprites.len(), 24);
    for (name, draws) in sprites {
        let actor = actors
            .iter()
            .find(|actor| actor["name"] == name.as_str())
            .expect("native pose has the same named child");
        assert_eq!(&actor["draws"], draws, "native geometry for {name}");
    }
}

#[test]
fn preserves_stable_post_draw_order() {
    let output = run_fixture("draw-order.json");
    let names: Vec<_> = output["samples"][0]["draw_sequence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "underlay",
            "decoration",
            "overlay_late_input",
            "overlay_equal_a",
            "overlay_equal_b"
        ]
    );
}

#[test]
fn evaluates_every_native_tween_curve() {
    let output = run_fixture("tween-curves.json");
    let actors = output["samples"][0]["actors"].as_array().unwrap();
    for actor in &actors[1..] {
        assert_eq!(
            actor["tween_queue"][0]["curve_probe"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
    }
    assert_eq!(actors[1]["tween_queue"][0]["curve_probe"][2], 0.5);
    assert_eq!(actors[2]["tween_queue"][0]["curve_probe"][2], 0.25);
    assert_eq!(actors[3]["tween_queue"][0]["curve_probe"][2], 0.75);
}

#[test]
fn command_appends_tweens_at_dispatch() {
    let output = run_fixture("scheduled-fade.json");
    let samples = output["samples"].as_array().unwrap();
    for actor in samples[0]["actors"].as_array().unwrap().iter().skip(1) {
        assert_eq!(actor["tween_queue"].as_array().unwrap().len(), 3);
        assert_eq!(actor["played_commands"], serde_json::json!([]));
    }
    let dispatched = samples
        .iter()
        .find(|sample| sample["time"].as_f64().unwrap() >= 1.1)
        .unwrap();
    for actor in dispatched["actors"].as_array().unwrap().iter().skip(1) {
        assert_eq!(actor["played_commands"], serde_json::json!(["Begin"]));
    }
    assert_eq!(dispatched["actors"][2]["tween_queue"][0]["duration"], 3.0);
    let alpha = dispatched["actors"][2]["current"]["diffuse"][0][3]
        .as_f64()
        .unwrap();
    assert!(alpha > 0.0 && alpha < 1.0);
}

#[test]
fn zero_time_reset_uses_float_frame_clock() {
    let output = run_fixture("queued-reset-clock.json");
    assert_eq!(
        output["samples"][180]["actors"][1]["played_commands"],
        serde_json::json!(["Shrink"])
    );
    assert!(
        output["samples"][263]["actors"][1]["current"]["position"][0]
            .as_f64()
            .unwrap()
            < 0.0
    );
    assert!(
        output["samples"][263]["actors"][2]["current"]["position"][0]
            .as_f64()
            .unwrap()
            < 0.0
    );
    assert_eq!(
        output["samples"][264]["actors"][1]["current"]["position"][0],
        1120.0
    );
    assert_eq!(
        output["samples"][264]["actors"][2]["current"]["position"][0],
        832.0
    );
    for actor in output["samples"][264]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
    {
        assert_eq!(actor["tween_queue"], serde_json::json!([]));
    }
}

#[test]
fn broadcasts_messages_on_native_queue_frames() {
    let output = run_fixture("queued-broadcasts.json");
    let samples = output["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 181);
    assert!(samples[0]["actors"][1].get("broadcast_commands").is_none());
    for (frame, names) in [
        (1, vec!["Zero"]),
        (13, vec!["Zero", "Hit"]),
        (22, vec!["Zero", "Hit", "Again"]),
    ] {
        assert_eq!(
            samples[frame]["actors"][1]["broadcast_commands"],
            serde_json::json!(names)
        );
        assert_eq!(
            samples[frame]["actors"][3]["broadcast_commands"],
            serde_json::json!(names)
        );
    }
    assert_eq!(samples[13]["actors"][1]["current"]["position"][0], 160.0);
    assert_eq!(
        samples[13]["actors"][3]["current"]["position"][0],
        453.3333435058594
    );
    assert_eq!(samples[0]["actors"][4]["current"]["position"][0], 0.0);
    assert_eq!(samples[1]["actors"][4]["current"]["position"][0], 50.0);
    assert_eq!(
        samples[37]["actors"][2]["played_commands"],
        serde_json::json!(["Cancel"])
    );
    assert_eq!(
        samples[180]["actors"][1]["broadcast_commands"],
        serde_json::json!(["Zero", "Hit", "Again"])
    );
    assert_eq!(samples[180]["actors"][1]["current"]["position"][1], 200.0);
    assert_eq!(samples[180]["actors"][3]["current"]["position"][1], 200.0);
}

#[test]
fn message_lookup_reaches_unnamed_native_child() {
    let output = run_fixture("unnamed-broadcast.json");
    let samples = output["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 61);
    assert_eq!(
        samples[0]["actors"][2]["current"]["diffuse"][0],
        serde_json::json!([1.0, 1.0, 1.0, 1.0])
    );
    for sample in &samples[1..] {
        assert_eq!(
            sample["actors"][2]["current"]["diffuse"][0],
            serde_json::json!([0.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            sample["actors"][3]["current"]["diffuse"][0],
            serde_json::json!([1.0, 1.0, 1.0, 1.0])
        );
    }
}
