use std::fmt;
use std::path::Path;

pub fn evaluate(path: &Path) -> Result<serde_json::Value, Error> {
    let request = std::fs::read(path).map_err(Error::Read)?;
    let mut request: serde_json::Value = serde_json::from_slice(&request).map_err(Error::Json)?;
    if let Some(root) = request.get_mut("root") {
        resolve_model_paths(root, path.parent().unwrap_or(Path::new(".")));
    }
    let request = serde_json::to_vec(&request).map_err(Error::Json)?;
    let response = native_eval(&request)?;
    let mut document: serde_json::Value = serde_json::from_slice(&response).map_err(Error::Json)?;
    if let Some(message) = document.get("error").and_then(serde_json::Value::as_str) {
        return Err(Error::Native(message.to_owned()));
    }
    document["provenance"] = serde_json::json!({
        "source_root": env!("ITGMANIA_BUILD_ROOT"),
        "git_revision": env!("ITGMANIA_GIT_REVISION"),
        "git_dirty": env!("ITGMANIA_GIT_DIRTY"),
        "execution": "embedded_native_sources"
    });
    Ok(document)
}

// Model pieces belong to the fixture directory, like their native material
// textures. Keep fixture requests portable across working directories.
fn resolve_model_paths(actor: &mut serde_json::Value, directory: &Path) {
    if actor["kind"] == "model" {
        if let Some(pieces) = actor
            .get_mut("model_paths")
            .and_then(serde_json::Value::as_array_mut)
        {
            for piece in pieces {
                if let Some(path) = piece.as_str() {
                    if Path::new(path).is_relative() {
                        *piece = directory.join(path).to_string_lossy().as_ref().into();
                    }
                }
            }
        }
    }
    if let Some(children) = actor
        .get_mut("children")
        .and_then(serde_json::Value::as_array_mut)
    {
        for child in children {
            resolve_model_paths(child, directory);
        }
    }
}

#[cfg(itgmania_oracle)]
fn native_eval(request: &[u8]) -> Result<Vec<u8>, Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_eval_actor_fixture(request: *const u8, request_len: usize) -> NativeBuffer;
        fn itg_oracle_free(data: *mut u8);
    }

    // SAFETY: the request slice remains alive during the call and its explicit
    // length prevents the native parser from reading beyond it.
    let native = unsafe { itg_oracle_eval_actor_fixture(request.as_ptr(), request.len()) };
    if native.data.is_null() {
        return Err(Error::Native("native actor oracle returned no data".into()));
    }
    // SAFETY: the bridge owns `len` initialized bytes at `data` until the
    // matching bridge free below.
    let bytes = unsafe { std::slice::from_raw_parts(native.data, native.len) }.to_vec();
    // SAFETY: this pointer was returned by the bridge and has not been freed.
    unsafe { itg_oracle_free(native.data) };
    Ok(bytes)
}

#[cfg(not(itgmania_oracle))]
fn native_eval(_request: &[u8]) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
}

#[derive(Debug)]
pub enum Error {
    Read(std::io::Error),
    Native(String),
    Json(serde_json::Error),
    #[cfg(not(itgmania_oracle))]
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "could not read actor fixture: {error}"),
            Self::Native(message) => write!(formatter, "ITGmania actor fixture failed: {message}"),
            Self::Json(error) => write!(
                formatter,
                "native actor oracle returned invalid JSON: {error}"
            ),
            #[cfg(not(itgmania_oracle))]
            Self::Unavailable => write!(
                formatter,
                "the embedded ITGmania actor oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(all(test, itgmania_oracle))]
mod tests {
    use super::*;

    #[test]
    fn model_texture_request() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/actors/model-texture-request.json");
        let result = evaluate(&path).expect("native Model texture request");
        let draws = result["samples"][0]["actors"][1]["draws"].as_array().unwrap();
        assert_eq!(draws.len(), 3);
        for draw in draws {
            assert_eq!(draw["texture_request"], serde_json::json!({
                "stretch": true, "mipmaps": true, "hot_pink_color_key": true
            }));
            // ModelTypes requests stretch for a 5x9 source. The native
            // RageBitmapTexture branch sets image size to the 8x16 allocation.
            assert_eq!(draw["texture_dimensions"], serde_json::json!({
                "source": [5, 9], "image": [8, 16], "texture": [8, 16]
            }));
        }
    }

    #[test]
    fn model_geometry_uses_native_loader_and_draw() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/model-geometry.json");
        let result = evaluate(&path).expect("native Model load and draw");
        assert_eq!(result, evaluate(&path).expect("repeat native Model load"));
        for (index, sample) in result["samples"].as_array().unwrap().iter().enumerate() {
            let draws = sample["actors"][1]["draws"].as_array().unwrap();
            assert_eq!(draws.len(), 2, "diffuse and glow");
            assert_eq!(draws[0]["model_mesh_name"], "Triangle");
            assert_eq!(draws[0]["primitive"], "triangles");
            assert_eq!(draws[0]["texture_mode"], "modulate");
            assert_eq!(draws[1]["texture_mode"], "glow");
            assert_eq!(
                draws[0]["normals"],
                serde_json::json!([[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]])
            );
            assert_eq!(
                draws[0]["texture_matrix_scale"],
                serde_json::json!([[1.0, 1.0], [0.0, 1.0], [1.0, 0.0]])
            );
            let origin = if index == 0 {
                [100.0, 200.0]
            } else {
                [120.0, 210.0]
            };
            for draw in draws {
                assert_eq!(draw["vertices"].as_array().unwrap().len(), 3);
                for (vertex, offset) in draw["vertices"].as_array().unwrap().iter().zip([
                    [0.0, 0.0, 0.0],
                    [20.0, 0.0, 0.0],
                    [0.0, -40.0, 10.0],
                ]) {
                    assert_eq!(
                        vertex["world"],
                        serde_json::json!([
                            origin[0] + offset[0],
                            origin[1] + offset[1],
                            offset[2],
                            1.0
                        ])
                    );
                }
            }
            for (actual, expected) in draws[0]["material"]["diffuse"]
                .as_array()
                .unwrap()
                .iter()
                .zip([0.4, 0.525, 0.6, 0.6])
            {
                assert!((actual.as_f64().unwrap() - expected).abs() < 0.000001);
            }
            assert_eq!(
                draws[1]["material"]["diffuse"],
                serde_json::json!([1.0, 0.0, 0.0, 0.25])
            );
            assert_eq!(sample["actors"][2]["draws"], serde_json::json!([]));
        }
    }

    #[test]
    fn sprite_methods_use_native() {
        for name in ["sprite-load", "texture-path"] {
            let path =
                Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/actors/{name}.json"));
            let result = evaluate(&path).expect("native Sprite assertions");
            for key in ["script_errors", "diagnostics", "allocations"] {
                assert_eq!(result[key], serde_json::json!([]), "{name}: {key}");
            }
        }
    }

    #[test]
    fn bitmap_methods_use_native_userdata() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/bitmap-methods.json");
        let result = evaluate(&path).expect("native BitmapText assertions");
        assert_eq!(result, evaluate(&path).expect("repeat native BitmapText"));
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
        assert_eq!(result["allocations"], serde_json::json!([]));
    }

    #[test]
    fn definitions_use_native_fallback_concatenation() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/definition-concat.json");
        let result = evaluate(&path).expect("native definition concatenation");
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn value_iterator_uses_native_fallback() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/value-iterator.json");
        let result = evaluate(&path).expect("native value iterator");
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn proxy_methods_use_native_userdata() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/proxy-methods.json");
        let result = evaluate(&path).expect("native ActorProxy assertions");
        assert_eq!(result, evaluate(&path).expect("repeat native ActorProxy"));
        assert_eq!(result["script_errors"], serde_json::json!([]));
        assert_eq!(result["diagnostics"], serde_json::json!([]));
        assert_eq!(result["allocations"], serde_json::json!([]));
    }

    #[test]
    fn aft_creation_uses_native_allocation_and_lua_methods() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/aft-creation.json");
        let result = evaluate(&path).expect("native AFT allocation assertions");
        assert_eq!(
            result,
            evaluate(&path).expect("repeat native AFT allocation")
        );
        assert_eq!(
            result["allocations"],
            serde_json::json!([
                {"width":65, "height":33, "alpha":true, "depth":true, "float":true},
                {"width":32, "height":16, "alpha":false, "depth":false, "float":false},
                {"width":1, "height":1, "alpha":false, "depth":false, "float":false}
            ])
        );
        assert_eq!(
            result["diagnostics"],
            serde_json::json!([
                "ActorFrameTexture: Cannot have width or height less than 1",
                "Can't Create an already created ActorFrameTexture",
                "ActorFrameTexture: Texture Name already in use."
            ])
        );
        assert_eq!(result["script_errors"], result["diagnostics"]);
    }

    #[test]
    fn queued_size_and_zero_tween_use_native_actors() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/runtime-size-zoom.json");
        let native = evaluate(&path).expect("native queued size");
        assert_eq!(native, evaluate(&path).expect("repeat queued size"));
        for sample in native["samples"].as_array().unwrap() {
            let second = sample["time"].as_f64().unwrap();
            let resized = &sample["actors"][1];
            let expected = if second < 1.0 {
                [1280.0, 720.0]
            } else {
                [854.0, 480.0]
            };
            assert_eq!(resized["size"], serde_json::json!(expected));
            assert_eq!(
                resized["played_commands"].as_array().unwrap().len(),
                usize::from(second >= 1.0)
            );
            if second >= 1.1 {
                assert_eq!(
                    sample["actors"][2]["current"]["zoom"],
                    serde_json::json!([0.0, 0.0, 0.0])
                );
            }
        }
    }

    #[test]
    fn queued_visibility_waits_for_positive_updates() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/queued-visibility.json");
        let native = evaluate(&path).expect("native queued commands");
        assert_eq!(native, evaluate(&path).expect("repeat queued commands"));
        for (index, sample) in native["samples"].as_array().unwrap().iter().enumerate() {
            for actor in sample["actors"].as_array().unwrap().iter().skip(1) {
                let name = actor["name"].as_str().unwrap();
                let expected = match name {
                    "QueuedHide" | "QueuedBuilder" => index < 2,
                    "QueuedShow" => index >= 2,
                    "DelayedRestore" => index < 2 || index >= 5,
                    "ImmediateHide" => false,
                    _ => panic!("unexpected actor {name}"),
                };
                assert_eq!(actor["visible"], expected, "{name} at {}", sample["time"]);
                if name == "ImmediateHide" {
                    continue;
                }
                let count = if index < 2 {
                    0
                } else if name == "DelayedRestore" && index >= 5 {
                    2
                } else {
                    1
                };
                assert_eq!(actor["played_commands"].as_array().unwrap().len(), count);
            }
        }
    }

    #[test]
    fn position_spline_native_samples_are_deterministic() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/position-spline.json");
        let first = evaluate(&path).expect("native spline sample");
        assert_eq!(first, evaluate(&path).expect("repeat native sample"));
        let cases = first["splines"].as_array().expect("spline cases");
        assert_eq!(cases.len(), 42);
        for case in cases {
            for sample in case["samples"].as_array().expect("samples") {
                for key in ["position", "derivative", "receptor"] {
                    assert!(
                        sample[key]
                            .as_array()
                            .expect("vector")
                            .iter()
                            .all(|value| value.as_f64().is_some_and(f64::is_finite))
                    );
                }
            }
        }
    }
}
