use std::fmt;
use std::path::Path;

pub fn evaluate(path: &Path) -> Result<serde_json::Value, Error> {
    let request = std::fs::read(path).map_err(Error::Read)?;
    let mut request: serde_json::Value = serde_json::from_slice(&request).map_err(Error::Json)?;
    if let Some(root) = request.get_mut("root") {
        resolve_model_paths(root, path.parent().unwrap_or(Path::new(".")));
    }
    for field in ["texture_files", "texture_headers"] {
        if let Some(files) = request
            .get_mut(field)
            .and_then(serde_json::Value::as_array_mut)
        {
            let directory = path.parent().unwrap_or(Path::new("."));
            for spec in files {
                if let Some(file) = spec.get_mut("file") {
                    if let Some(relative) =
                        file.as_str().filter(|file| Path::new(file).is_relative())
                    {
                        *file = directory.join(relative).to_string_lossy().as_ref().into();
                    }
                }
            }
        }
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

/// Probe the actual native file loader rather than an image's logical canvas.
pub fn texture_source_size(path: &Path) -> Result<[u32; 2], Error> {
    let request = serde_json::to_vec(&serde_json::json!({
        "texture_headers": [{"file": path.to_string_lossy()}]
    }))
    .map_err(Error::Json)?;
    let response = native_eval(&request)?;
    let value: serde_json::Value = serde_json::from_slice(&response).map_err(Error::Json)?;
    if let Some(message) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(Error::Native(message.to_owned()));
    }
    let dimension = |axis: usize| {
        value["cases"][0]["dimensions"][axis]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or_else(|| Error::Native("invalid native texture dimensions".into()))
    };
    Ok([dimension(0)?, dimension(1)?])
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
    fn indexed_headers_match_native_bitmap_and_regular_model() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors");
        let headers = evaluate(&root.join("indexed-texture-headers.json")).expect("native headers");
        let files =
            evaluate(&root.join("indexed-bitmap-files.json")).expect("actual native bitmaps");
        for (index, name) in ["frame-offset.gif", "os2-4.bmp"].iter().enumerate() {
            let dimensions =
                texture_source_size(&root.join("bitmap-loader").join(name)).expect("native size");
            assert_eq!(dimensions, [8, 8]);
            assert_eq!(
                headers["cases"][index]["dimensions"],
                serde_json::json!(dimensions)
            );
            assert_eq!(
                files["cases"][index]["dimensions"]["source"],
                serde_json::json!(dimensions)
            );
        }
        let model = evaluate(&root.join("indexed-model-texture-profile.json"))
            .expect("regular Model GIF texture");
        for draw in model["samples"][0]["actors"][1]["draws"]
            .as_array()
            .unwrap()
        {
            assert_eq!(draw["texture_dimensions"], files["cases"][0]["dimensions"]);
        }
        let error =
            texture_source_size(&root.join("bitmap-loader/native-misnamed-bmp.png")).unwrap_err();
        assert!(error.to_string().contains("Unknown file format"), "{error}");
    }

    #[test]
    fn normal_model_texture_profile_matches_native_bitmap() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors");
        let actor = evaluate(&root.join("model-game-texture-profile.json"))
            .expect("native Model with a texture above the constructor cap");
        let bitmap = evaluate(&root.join("bitmap-loader.json")).expect("native bitmap profiles");
        let file = bitmap["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == "model-game-default-cap")
            .unwrap();
        assert_eq!(file["profile"]["color_depth"], 32);
        assert_eq!(file["profile"]["max_size"], 2048);
        assert_eq!(file["dimensions"]["image"], serde_json::json!([2048, 8]));
        let draws = actor["samples"][0]["actors"][1]["draws"]
            .as_array()
            .unwrap();
        assert_eq!(draws.len(), 3);
        for draw in draws {
            assert_eq!(draw["texture_dimensions"], file["dimensions"]);
        }
    }

    #[test]
    fn bitmap_loader_decodes_native_files() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/bitmap-loader.json");
        let result = evaluate(&path).expect("native bitmap file decoding");
        assert_eq!(
            result,
            evaluate(&path).expect("repeat native file decoding")
        );
        assert_eq!(result["oracle"], "itgmania_native_bitmap_loader");
        assert_eq!(result["framebuffer_verified"], false);
        let cases = result["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 28);
        let case = |name: &str| cases.iter().find(|case| case["name"] == name).unwrap();
        assert_eq!(
            case("model-pink")["upload"]["pixels"],
            serde_json::json!(vec![[0; 4]; 128])
        );
        assert_eq!(case("unicode-path")["upload"], case("model-pink")["upload"]);
        assert_eq!(case("misnamed-png")["upload"], case("model-pink")["upload"]);
        assert_eq!(
            case("sprite-pink")["loaded"]["pixels"],
            serde_json::json!(vec![[255, 0, 255, 255]; 45])
        );
        assert_eq!(
            case("sprite-pink")["dimensions"],
            serde_json::json!({
                "source": [5, 9], "image": [5, 9], "texture": [8, 16]
            })
        );
        // Native Blit initializes only one extra border row/column. The rest
        // of a Sprite's allocation is intentionally excluded from evidence.
        assert_eq!(case("sprite-pink")["upload"]["pixels_captured"], false);
        assert!(case("sprite-pink")["upload"].get("pixels").is_none());
        assert_eq!(case("native-png16")["loaded"]["surface_bit_depth"], 32);
        assert_eq!(
            case("native-png16")["loaded"]["pixels"][0],
            serde_json::json!([18, 128, 254, 255])
        );
        for name in ["bmp4-key", "bmp8-key", "gif-key"] {
            let decoded = case(name);
            assert_eq!(decoded["loaded"]["surface_bit_depth"], 8);
            assert_eq!(
                decoded["loaded"]["pixels"][0],
                serde_json::json!([255, 0, 255, 255])
            );
            let pixels = &decoded["upload"]["pixels"];
            // The pinned BMP4 loader reads the low nibble first, unlike the
            // standard. Record that native behavior without rewriting input.
            let (key, duplicate, red) = if name == "bmp4-key" {
                (1, 0, 3)
            } else {
                (0, 1, 2)
            };
            assert_eq!(pixels[key], serde_json::json!([0, 0, 0, 0]));
            assert_eq!(pixels[duplicate], serde_json::json!([255, 0, 255, 255]));
            assert_eq!(pixels[red], serde_json::json!([240, 40, 10, 255]));
        }
        assert_eq!(case("bmp-pal-supported")["upload"]["surface_bit_depth"], 8);
        assert_eq!(
            case("bmp-pal-supported")["upload"]["requested_pixel_format"],
            "PAL"
        );
        assert_eq!(case("native-jpeg")["loaded"]["width"], 8);
        let jpeg = case("native-jpeg")["loaded"]["pixels"].as_array().unwrap();
        assert!(jpeg.iter().all(|pixel| pixel == &jpeg[0]));
        assert_eq!(jpeg[0][3], 255);
        for name in ["sprite-bmp8", "sprite-gif", "sprite-jpeg", "sprite-gray"] {
            let sprite = case(name);
            assert_eq!(sprite["upload"]["pixels_captured"], true);
            assert_eq!(
                sprite["dimensions"]["image"],
                sprite["dimensions"]["texture"]
            );
            assert_eq!(sprite["upload"]["pixels"].as_array().unwrap().len(), 64);
        }
        assert_eq!(
            case("sprite-bmp8")["upload"]["pixels"][0],
            serde_json::json!([255, 0, 255, 255])
        );
        assert_eq!(
            case("sprite-gif")["upload"]["pixels"][3],
            serde_json::json!([0, 0, 0, 0])
        );
    }

    #[test]
    fn bitmap_loader_applies_native_preferences_and_hints() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/bitmap-loader.json");
        let result = evaluate(&path).expect("native bitmap policy");
        let cases = result["cases"].as_array().unwrap();
        let case = |name: &str| cases.iter().find(|case| case["name"] == name).unwrap();
        let defaults = case("manager-constructor-defaults");
        assert_eq!(defaults["adjusted_id"]["max_size"], 1024);
        assert_eq!(defaults["adjusted_id"]["color_depth"], 16);
        assert_eq!(case("model-pink")["adjusted_id"]["max_size"], 2048);
        assert_eq!(case("model-pink")["adjusted_id"]["color_depth"], 32);
        assert_eq!(
            case("model-game-default-cap")["dimensions"]["texture"],
            serde_json::json!([2048, 8])
        );
        assert_eq!(
            case("model-constructor-cap")["dimensions"]["texture"],
            serde_json::json!([1024, 8])
        );
        assert_eq!(case("model-id-cap")["adjusted_id"]["max_size"], 2048);
        assert_eq!(case("model-nomipmaps")["adjusted_id"]["mipmaps"], true);
        assert_eq!(
            case("model-nomipmaps")["upload"]["requested_mipmaps"],
            false
        );
        assert_eq!(
            case("sprite-force-mipmaps")["upload"]["requested_mipmaps"],
            true
        );
        assert_eq!(case("filename-16bpp")["adjusted_id"]["color_depth"], 32);
        assert_eq!(
            case("filename-16bpp")["upload"]["requested_pixel_format"],
            "RGB5A1"
        );
        assert_eq!(
            case("hires-on")["dimensions"]["texture"],
            serde_json::json!([16, 16])
        );
        assert_eq!(
            case("hires-off")["dimensions"]["texture"],
            serde_json::json!([8, 8])
        );
        for name in ["grayscale-supported", "alphamap-supported"] {
            assert_eq!(case(name)["upload"]["surface_bit_depth"], 8);
            assert_eq!(case(name)["upload"]["requested_pixel_format"], "PAL");
        }
        assert_eq!(
            case("grayscale-no-palette")["upload"]["requested_pixel_format"],
            "RGBA8"
        );
        assert_eq!(case("dither16")["upload"]["surface_bit_depth"], 16);
        assert_eq!(
            case("dither16")["upload"]["requested_pixel_format"],
            "RGBA4"
        );
        assert_eq!(
            case("alphamap-supported")["upload"]["pixels"][32],
            serde_json::json!([255, 255, 255, 136])
        );
    }

    #[test]
    fn bitmap_loader_rejects_invalid_profiles_and_restores_state() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/actors/bitmap-loader/model-pink-npot.png");
        for (key, value) in [
            ("color_depth", serde_json::json!(24)),
            ("max_size", serde_json::json!(7)),
            ("max_size", serde_json::json!(9)),
            ("max_size", serde_json::json!(8192)),
            ("high_resolution", serde_json::json!(1)),
            ("kind", serde_json::json!("movie")),
        ] {
            let mut case = serde_json::json!({"file": file, "kind": "model"});
            case[key] = value;
            let request =
                serde_json::to_vec(&serde_json::json!({"texture_files": [case]})).unwrap();
            let result: serde_json::Value =
                serde_json::from_slice(&native_eval(&request).unwrap()).unwrap();
            assert!(
                result["error"].is_string(),
                "accepted invalid {key}: {result}"
            );
        }
        // Adjusted custom IDs must be removed using their registered pointer.
        // Repeated AFT creation followed by bitmap mode catches stale entries.
        let actors =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/aft-creation.json");
        let files =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/bitmap-loader.json");
        evaluate(&actors).expect("native AFT after rejected bitmap input");
        evaluate(&actors).expect("native AFT registry cleanup");
        evaluate(&files).expect("bitmap registry is empty after AFT deletion");
    }

    #[test]
    fn texture_surface_uses_native_preprocessing() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/actors/texture-surface.json");
        let result = evaluate(&path).expect("native surface preprocessing");
        assert_eq!(
            result,
            evaluate(&path).expect("repeat native preprocessing")
        );
        assert_eq!(result["oracle"], "itgmania_native_surface_utils");
        assert_eq!(result["framebuffer_verified"], false);
        let cases = result["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 12);
        let pixels =
            |name: &str| &cases.iter().find(|case| case["name"] == name).unwrap()["output_pixels"];
        assert_eq!(
            pixels("model-pink-npot"),
            &serde_json::json!(vec![[0; 4]; 128])
        );
        assert_eq!(
            pixels("off-pink-top-edge")[0],
            serde_json::json!([255, 0, 255, 255])
        );
        assert_eq!(
            pixels("off-pink-top-edge")[1],
            serde_json::json!([0, 0, 0, 0])
        );
        assert_eq!(
            pixels("off-pink-side-middle")[3],
            serde_json::json!([248, 0, 248, 255])
        );
        assert_eq!(
            pixels("off-pink-side-middle")[4],
            serde_json::json!([240, 40, 10, 0])
        );
        assert_eq!(
            pixels("partial-alpha-pink")[0],
            serde_json::json!([255, 0, 255, 128])
        );
        assert_eq!(
            pixels("rgb-key-adds-alpha")[0],
            serde_json::json!([240, 40, 10, 0])
        );
        assert_eq!(
            pixels("palette-keys-both-pinks")[0],
            serde_json::json!([240, 40, 10, 0])
        );
        assert_eq!(
            pixels("palette-keys-both-pinks")[1],
            serde_json::json!([240, 40, 10, 0])
        );
        assert_eq!(
            pixels("transparent-hidden-rgb"),
            &serde_json::json!(vec![[0; 4]; 4])
        );
        assert_eq!(
            pixels("uniform-hidden-rgb")[1],
            serde_json::json!([240, 40, 10, 0])
        );
        assert_eq!(
            pixels("mixed-hidden-rgb")[1],
            serde_json::json!([0, 0, 0, 0])
        );
        assert_eq!(pixels("npot-linear-stretch").as_array().unwrap().len(), 128);
        assert_eq!(
            pixels("npot-linear-stretch")[0],
            serde_json::json!([0, 0, 0, 255])
        );
        assert_eq!(
            pixels("npot-linear-stretch")[127],
            serde_json::json!([200, 224, 228, 255])
        );
        assert_eq!(pixels("iterative-reduction").as_array().unwrap().len(), 1);
        assert_eq!(
            pixels("one-pixel-expansion"),
            &serde_json::json!(vec![[240, 40, 10, 255]; 64])
        );
    }

    #[test]
    fn texture_surface_rejects_invalid_inputs() {
        let valid = serde_json::json!({"width": 1, "height": 1, "pixels": [[0, 0, 0, 255]]});
        for (key, value) in [
            ("width", serde_json::json!(0)),
            ("height", serde_json::json!(257)),
            ("width", serde_json::json!(1.5)),
            ("format", serde_json::json!("unknown")),
            ("pixels", serde_json::json!([[0, 0, 0, 256]])),
            ("pixels", serde_json::json!([])),
            ("destination", serde_json::json!([0, 8])),
            ("destination", serde_json::json!([8])),
            ("destination", serde_json::json!([4096, 4096])),
            ("hot_pink_color_key", serde_json::json!(1)),
        ] {
            let mut case = valid.clone();
            case[key] = value;
            let request =
                serde_json::to_vec(&serde_json::json!({"texture_surface": [case]})).unwrap();
            let response: serde_json::Value =
                serde_json::from_slice(&native_eval(&request).unwrap()).unwrap();
            assert!(
                response["error"].is_string(),
                "accepted invalid {key}: {response}"
            );
        }
    }

    #[test]
    fn model_texture_request() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/actors/model-texture-request.json");
        let result = evaluate(&path).expect("native Model texture request");
        let draws = result["samples"][0]["actors"][1]["draws"]
            .as_array()
            .unwrap();
        assert_eq!(draws.len(), 3);
        for draw in draws {
            assert_eq!(
                draw["texture_request"],
                serde_json::json!({
                    "stretch": true, "mipmaps": true, "hot_pink_color_key": true
                })
            );
            // ModelTypes requests stretch for a 5x9 source. The native
            // RageBitmapTexture branch sets image size to the 8x16 allocation.
            assert_eq!(
                draw["texture_dimensions"],
                serde_json::json!({
                    "source": [5, 9], "image": [8, 16], "texture": [8, 16]
                })
            );
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
