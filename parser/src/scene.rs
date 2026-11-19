use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;

use crate::{Project, read_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlendMode {
    Normal,
    Translucent,
    Additive,
    Opaque,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageLayer {
    pub id: i64,
    pub name: String,
    pub origin: [f32; 3],
    pub size: [f32; 2],
    pub scale: [f32; 3],
    /// Euler angles in degrees, as stored in Wallpaper Engine scene files.
    pub angles: [f32; 3],
    pub alpha: f32,
    pub color: [f32; 3],
    pub brightness: f32,
    pub visible: bool,
    /// Fully resolved resource path, suitable for AssetStore::read().
    pub texture: String,
    pub shader: String,
    pub blending: BlendMode,
    pub source: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct SceneDocument {
    pub width: u32,
    pub height: u32,
    pub clear_color: [f32; 4],
    pub camera: SceneCamera,
    pub bloom: Option<BloomSettings>,
    pub images: Vec<ImageLayer>,
    pub warnings: Vec<String>,
    pub source: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct BloomSettings {
    pub strength: f32,
    pub threshold: f32,
    pub tint: [f32; 3],
}

/// Orthographic world-to-canvas rows, including camera translation.
#[derive(Debug, Clone, Serialize)]
pub struct SceneCamera {
    pub rows: [[f32; 4]; 2],
}

impl SceneCamera {
    pub fn project(&self, point: [f32; 3]) -> [f32; 2] {
        self.rows
            .map(|row| row[0] * point[0] + row[1] * point[1] + row[2] * point[2] + row[3])
    }
}

pub(crate) fn load(project: &Project) -> Result<SceneDocument> {
    let source = read_json(&project.assets, &project.metadata.file)?;
    let general = source
        .get("general")
        .context("scene has no general settings")?;
    let projection = general
        .get("orthogonalprojection")
        .context("scene has no orthogonalprojection")?;
    ensure!(
        !projection
            .get("auto")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "automatic scene projection is not supported yet; an explicit width and height are required"
    );
    let width = dimension(projection, "width")?;
    let height = dimension(projection, "height")?;
    let mut warnings = Vec::new();
    let camera = parse_camera(&source, width, height, project, &mut warnings)?;
    let clear = vector::<3>(
        general.get("clearcolor"),
        [0.0; 3],
        project,
        &mut warnings,
        "clearcolor",
    )?;
    // Camera fade belongs to global camera path transitions. It is inert for
    // a static camera, even when enabled in the editor's default settings.
    let _camera_fade = boolean(
        general.get("camerafade"),
        true,
        project,
        &mut warnings,
        "camerafade",
    )?;
    if source
        .pointer("/camera/paths")
        .and_then(Value::as_array)
        .is_some_and(|v| !v.is_empty())
    {
        warnings.push("global camera paths and their transition fades are not implemented".into());
    }
    let bloom = if boolean(general.get("bloom"), false, project, &mut warnings, "bloom")? {
        if boolean(general.get("hdr"), false, project, &mut warnings, "hdr")? {
            warnings.push("HDR scene bloom is not implemented; only SDR bloom is supported".into());
        }
        Some(BloomSettings {
            strength: scalar(
                general.get("bloomstrength"),
                2.,
                project,
                &mut warnings,
                "bloomstrength",
            )?
            .max(0.),
            threshold: scalar(
                general.get("bloomthreshold"),
                0.65,
                project,
                &mut warnings,
                "bloomthreshold",
            )?,
            tint: vector(
                general.get("bloomtint"),
                [1.; 3],
                project,
                &mut warnings,
                "bloomtint",
            )?,
        })
    } else {
        None
    };
    for field in ["cameraparallax", "camerashake"] {
        if boolean(general.get(field), false, project, &mut warnings, field)? {
            warnings.push(format!("scene {field} is not implemented"));
        }
    }
    let objects = source
        .get("objects")
        .and_then(Value::as_array)
        .context("scene has no objects array")?;
    let mut images = Vec::new();
    for object in objects {
        let id = object.get("id").and_then(Value::as_i64).unwrap_or(-1);
        let label = object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed");
        let Some(model_path) = object.get("image").and_then(Value::as_str) else {
            let kind = ["particle", "sound", "text", "model", "light"]
                .into_iter()
                .find(|key| object.get(key).is_some())
                .unwrap_or("object/group");
            warnings.push(format!("object {id} ({label}): {kind} is not implemented"));
            continue;
        };
        let image = parse_image(project, object, model_path, &mut warnings)
            .with_context(|| format!("cannot load image object {id} ({label})"))?;
        images.push(image);
    }
    Ok(SceneDocument {
        width,
        height,
        clear_color: [clear[0], clear[1], clear[2], 1.0],
        camera,
        bloom,
        images,
        warnings,
        source,
    })
}

fn parse_image(
    project: &Project,
    object: &Value,
    model_path: &str,
    warnings: &mut Vec<String>,
) -> Result<ImageLayer> {
    let model = read_json(&project.assets, model_path)?;
    let material_path = model
        .get("material")
        .and_then(Value::as_str)
        .context("image model has no material path")?;
    let material = read_json(&project.assets, material_path)?;
    let passes = material
        .get("passes")
        .and_then(Value::as_array)
        .context("material has no passes array")?;
    let pass = passes.first().context("material has no rendering pass")?;
    let id = object.get("id").and_then(Value::as_i64).unwrap_or(-1);
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unnamed")
        .to_owned();
    let prefix = format!("object {id} ({name})");
    if passes.len() > 1 {
        warnings.push(format!(
            "{prefix}: only the first material pass is supported"
        ));
    }
    for field in ["effects", "animationlayers", "dependencies"] {
        if object
            .get(field)
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        {
            warnings.push(format!("{prefix}: {field} are not implemented"));
        }
    }
    if object.get("parent").is_some_and(|value| !value.is_null()) {
        warnings.push(format!("{prefix}: parent transforms are not implemented"));
    }
    if model.get("puppet").is_some() {
        warnings.push(format!("{prefix}: puppet deformation is not implemented"));
    }
    for field in ["fullscreen", "passthrough", "solidlayer"] {
        if model.get(field).and_then(Value::as_bool).unwrap_or(false) {
            warnings.push(format!("{prefix}: model flag {field} is not implemented"));
        }
    }
    let shader = pass
        .get("shader")
        .and_then(Value::as_str)
        .context("material pass has no shader")?
        .to_owned();
    if !["genericimage2", "genericimage"].contains(&shader.as_str()) {
        warnings.push(format!(
            "{prefix}: shader {shader:?} uses the basic image fallback"
        ));
    }
    if let Some(combos) = pass.get("combos").and_then(Value::as_object) {
        for (name, value) in combos {
            // VERSION selects the image color uniform ABI. Our image pipeline
            // already supplies the equivalent brightness, tint and alpha.
            let supported = ["genericimage", "genericimage2"].contains(&shader.as_str())
                && match name.as_str() {
                    "VERSION" => value.as_i64().is_some_and(|v| (0..=2).contains(&v)),
                    "LIGHTING" | "REFLECTION" | "NORMALMAP" | "PBRMASKS" | "BLENDMODE"
                    | "SKINNING" => value.as_i64() == Some(0),
                    _ => false,
                };
            if !supported {
                warnings.push(format!(
                    "{prefix}: material combo {name}={value} is not implemented"
                ));
            }
        }
    }
    if pass
        .get("constantshadervalues")
        .and_then(Value::as_object)
        .is_some_and(|v| !v.is_empty())
    {
        warnings.push(format!(
            "{prefix}: material shader constants are not implemented"
        ));
    }
    if pass.get("usertextures").is_some_and(has_values) {
        warnings.push(format!(
            "{prefix}: material usertextures are not implemented"
        ));
    }
    for (field, supported) in [
        ("depthtest", "disabled"),
        ("depthwrite", "disabled"),
        ("cullmode", "nocull"),
    ] {
        if pass
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|v| v != supported)
        {
            warnings.push(format!("{prefix}: material {field} is not implemented"));
        }
    }
    if object
        .get("instance")
        .and_then(Value::as_object)
        .is_some_and(|v| v.keys().any(|k| k != "textures"))
    {
        warnings.push(format!(
            "{prefix}: additional material instance overrides are not implemented"
        ));
    }
    for field in ["horizontalalign", "verticalalign", "alignment"] {
        if object
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|v| v != "center")
        {
            warnings.push(format!("{prefix}: image alignment is not implemented"));
        }
    }
    let textures = object
        .pointer("/instance/textures")
        .or_else(|| pass.get("textures"))
        .and_then(Value::as_array)
        .context("material has no textures array")?;
    if textures.iter().skip(1).any(|value| !value.is_null()) {
        warnings.push(format!(
            "{prefix}: additional material textures are not implemented"
        ));
    }
    let first = textures
        .first()
        .context("material has no primary texture")?;
    let texture_name = first
        .as_str()
        .or_else(|| first.get("name").and_then(Value::as_str))
        .context("material primary texture is not a filename")?;
    let texture = resolve_texture(project, texture_name)?;
    let blending = match pass
        .get("blending")
        .and_then(Value::as_str)
        .unwrap_or("normal")
    {
        "normal" => BlendMode::Normal,
        "translucent" => BlendMode::Translucent,
        "additive" => BlendMode::Additive,
        "disabled" | "opaque" => BlendMode::Opaque,
        name => bail!("unsupported material blending mode: {name}"),
    };
    let origin = vector(object.get("origin"), [0.0; 3], project, warnings, "origin")?;
    // Autosized models take their natural dimensions from the texture at upload
    // time. An explicit scene size still controls the displayed quad.
    let natural_size = if model.get("autosize").and_then(Value::as_bool) == Some(true) {
        [0.0; 2]
    } else {
        [
            scalar(model.get("width"), 0.0, project, warnings, "model.width")?,
            scalar(model.get("height"), 0.0, project, warnings, "model.height")?,
        ]
    };
    let size = vector(object.get("size"), natural_size, project, warnings, "size")?;
    let scale = vector(object.get("scale"), [1.0; 3], project, warnings, "scale")?;
    let angles = vector(object.get("angles"), [0.0; 3], project, warnings, "angles")?;
    let alpha = scalar(object.get("alpha"), 1.0, project, warnings, "alpha")?;
    let brightness = scalar(
        object.get("brightness"),
        1.0,
        project,
        warnings,
        "brightness",
    )?;
    if scalar(
        object.get("colorBlendMode"),
        0.0,
        project,
        warnings,
        "colorBlendMode",
    )? != 0.0
    {
        warnings.push(format!("{prefix}: colorBlendMode is not implemented"));
    }
    let color = color(object.get("color"), project, warnings)?;
    let visible = boolean(object.get("visible"), true, project, warnings, "visible")?;
    Ok(ImageLayer {
        id,
        name,
        origin,
        size,
        scale,
        angles,
        alpha,
        color,
        brightness,
        visible,
        texture,
        shader,
        blending,
        source: object.clone(),
    })
}

fn has_values(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|v| !v.is_null()),
        Value::Object(values) => !values.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

fn parse_camera(
    source: &Value,
    width: u32,
    height: u32,
    project: &Project,
    warnings: &mut Vec<String>,
) -> Result<SceneCamera> {
    let Some(camera) = source.get("camera") else {
        return Ok(SceneCamera {
            rows: [[1., 0., 0., 0.], [0., 1., 0., 0.]],
        });
    };
    let center = vector(
        camera.get("center"),
        [width as f32 / 2.0, height as f32 / 2.0, 0.0],
        project,
        warnings,
        "camera.center",
    )?;
    let eye = vector(
        camera.get("eye"),
        [center[0], center[1], center[2] + 1.0],
        project,
        warnings,
        "camera.eye",
    )?;
    let up = vector(
        camera.get("up"),
        [0.0, 1.0, 0.0],
        project,
        warnings,
        "camera.up",
    )?;
    let subtract = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| a[i] - b[i]);
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let normalize = |v: [f32; 3]| -> Result<[f32; 3]> {
        let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        ensure!(
            length > 0.000001,
            "camera has a degenerate view or up vector"
        );
        Ok(v.map(|x| x / length))
    };
    let backward = normalize(subtract(eye, center))?;
    let right = normalize(cross(up, backward))?;
    let camera_up = cross(backward, right);
    // Published scenes use an origin-based camera and canvas-centered object
    // coordinates. Also accept the explicit canvas-centered camera convention.
    let standard_center = (center[0] - width as f32 / 2.0).abs() < 0.001
        && (center[1] - height as f32 / 2.0).abs() < 0.001
        && center[2].abs() < 0.001;
    let half = [width as f32 / 2., height as f32 / 2.];
    let offset = if standard_center {
        [eye[0], eye[1], eye[2]]
    } else {
        [eye[0] + half[0], eye[1] + half[1], eye[2]]
    };
    let zoom = scalar(
        source.pointer("/general/zoom"),
        1.,
        project,
        warnings,
        "zoom",
    )?;
    ensure!(zoom > 0., "camera zoom must be positive");
    let rows = [right, camera_up].map(|axis| {
        [
            axis[0] * zoom,
            axis[1] * zoom,
            axis[2] * zoom,
            -axis.iter().zip(offset).map(|(a, b)| a * b).sum::<f32>() * zoom,
        ]
    });
    let mut rows = rows;
    rows[0][3] += half[0];
    rows[1][3] += half[1];
    for field in ["nearz", "farz"] {
        if camera.get(field).is_some() {
            warnings.push(format!("camera {field} clipping is not implemented"));
        }
    }
    Ok(SceneCamera { rows })
}

fn dimension(projection: &Value, field: &str) -> Result<u32> {
    let n = projection
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("scene projection has no positive integer {field}"))?;
    ensure!(
        (1..=32768).contains(&n),
        "scene {field} must be between 1 and 32768"
    );
    Ok(n as u32)
}

fn color(value: Option<&Value>, project: &Project, warnings: &mut Vec<String>) -> Result<[f32; 3]> {
    let Some(value) = effective(value, project, warnings, "color") else {
        return Ok([1.0; 3]);
    };
    let (mut values, bytes) = if let Some(text) = value.as_str() {
        if let Some(hex) = text.strip_prefix('#') {
            ensure!(
                [3, 4, 6, 8].contains(&hex.len()) && hex.is_ascii(),
                "invalid hexadecimal image color"
            );
            let expanded = if hex.len() <= 4 {
                hex.chars().flat_map(|c| [c, c]).collect::<String>()
            } else {
                hex.to_owned()
            };
            let channels = (0..expanded.len())
                .step_by(2)
                .map(|index| {
                    Ok::<_, anyhow::Error>(
                        u8::from_str_radix(&expanded[index..index + 2], 16)? as f32
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            (channels, true)
        } else {
            let values = text
                .replace(',', " ")
                .split_whitespace()
                .map(str::parse)
                .collect::<std::result::Result<Vec<f32>, _>>()?;
            // Wallpaper Engine's image color strings distinguish byte colors
            // from normalized colors using a decimal point.
            (values, !text.contains('.'))
        }
    } else if let Some(array) = value.as_array() {
        (
            array
                .iter()
                .map(|v| {
                    v.as_f64()
                        .map(|n| n as f32)
                        .context("image color must contain numbers")
                })
                .collect::<Result<Vec<_>>>()?,
            false,
        )
    } else {
        bail!("image color must be a color string or array")
    };
    ensure!(
        [3, 4].contains(&values.len()) && values.iter().all(|v| v.is_finite()),
        "image color requires 3 or 4 finite components"
    );
    if bytes {
        for value in &mut values {
            *value /= 255.0;
        }
    }
    if values.get(3).is_some_and(|a| (*a - 1.0).abs() > 0.001) {
        warnings.push("image color alpha is not implemented; use the alpha property".into());
    }
    Ok([values[0], values[1], values[2]])
}

fn resolve_texture(project: &Project, name: &str) -> Result<String> {
    let candidates = [
        name.to_owned(),
        format!("materials/{name}.tex"),
        format!("{name}.tex"),
    ];
    for path in &candidates {
        if project.assets.exists(path) {
            return Ok(path.clone());
        }
    }
    bail!("texture not found: {name}; tried {}", candidates.join(", "))
}

fn effective<'a>(
    value: Option<&'a Value>,
    project: &'a Project,
    warnings: &mut Vec<String>,
    field: &str,
) -> Option<&'a Value> {
    let value = value?;
    if !value.is_object() {
        return Some(value);
    }
    if value.get("script").is_some_and(|v| !v.is_null()) {
        warnings.push(format!(
            "property {field}: script is not implemented; using its initial value"
        ));
    }
    if let Some(user) = value.get("user").filter(|v| !v.is_null()) {
        if let Some(name) = user.as_str() {
            if let Some(property) = project.metadata.properties.get(name) {
                return Some(property.get("value").unwrap_or(property));
            }
            // Projects can retain bindings to properties deleted in the editor.
            // Their serialized value remains the fallback, as for an unbound field.
        } else {
            warnings.push(format!(
                "property {field}: conditional user binding is not implemented"
            ));
        }
    }
    value.get("value")
}

fn scalar(
    value: Option<&Value>,
    default: f32,
    project: &Project,
    warnings: &mut Vec<String>,
    field: &str,
) -> Result<f32> {
    let Some(value) = effective(value, project, warnings, field) else {
        return Ok(default);
    };
    let value = value
        .as_f64()
        .map(|n| n as f32)
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .with_context(|| format!("{field} must be a number"))?;
    ensure!(value.is_finite(), "{field} must be finite");
    Ok(value)
}

fn boolean(
    value: Option<&Value>,
    default: bool,
    project: &Project,
    warnings: &mut Vec<String>,
    field: &str,
) -> Result<bool> {
    let Some(value) = effective(value, project, warnings, field) else {
        return Ok(default);
    };
    value
        .as_bool()
        .or_else(|| value.as_i64().map(|n| n != 0))
        .with_context(|| format!("{field} must be boolean"))
}

fn vector<const N: usize>(
    value: Option<&Value>,
    default: [f32; N],
    project: &Project,
    warnings: &mut Vec<String>,
    field: &str,
) -> Result<[f32; N]> {
    let Some(value) = effective(value, project, warnings, field) else {
        return Ok(default);
    };
    let values: Vec<f32> = if let Some(text) = value.as_str() {
        text.split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()
            .with_context(|| format!("invalid vector in {field}"))?
    } else if let Some(array) = value.as_array() {
        array
            .iter()
            .map(|v| {
                v.as_f64()
                    .map(|v| v as f32)
                    .with_context(|| format!("{field} must contain numbers"))
            })
            .collect::<Result<_>>()?
    } else {
        bail!("{field} must be a vector string or array")
    };
    ensure!(
        values.len() == N,
        "{field} requires {N} components, found {}",
        values.len()
    );
    ensure!(
        values.iter().all(|v| v.is_finite()),
        "{field} must contain finite numbers"
    );
    Ok(values.try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rxwpe-parser-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("models")).unwrap();
            fs::create_dir_all(root.join("materials")).unwrap();
            Self(root)
        }
        fn write(&self, path: &str, value: Value) {
            fs::write(self.0.join(path), serde_json::to_vec(&value).unwrap()).unwrap();
        }
        fn project(&self) {
            self.write("project.json",serde_json::json!({"type":"scene","file":"scene.json","general":{"properties":{"opacity":{"type":"slider","value":0.25}}}}));
            self.write("scene.json",serde_json::json!({"general":{"orthogonalprojection":{"width":1920,"height":1080},"clearcolor":"0.1 0.2 0.3"},"objects":[{"image":"models/image.json","id":7,"name":"background","origin":"960 540 0","size":"1920 1080","alpha":{"value":1,"user":"opacity"}}]}));
            self.write(
                "models/image.json",
                serde_json::json!({"material":"materials/image.json"}),
            );
            self.write(
                "materials/image.json",
                serde_json::json!({"passes":[{"shader":"genericimage2","textures":["example"]}]}),
            );
            fs::write(self.0.join("materials/example.tex"), b"texture placeholder").unwrap();
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn follows_resource_references_and_user_overrides() {
        let dir = TestDir::new();
        dir.project();
        let mut project = Project::load(&dir.0, None).unwrap();
        let scene = project.scene().unwrap();
        assert_eq!((scene.width, scene.height), (1920, 1080));
        assert_eq!(scene.images[0].texture, "materials/example.tex");
        assert_eq!(scene.images[0].alpha, 0.25);
        assert_eq!(scene.images[0].origin, [960., 540., 0.]);
        assert!(scene.warnings.is_empty());
        project
            .set_property("opacity", serde_json::json!(0.75))
            .unwrap();
        assert_eq!(project.scene().unwrap().images[0].alpha, 0.75);
    }

    #[test]
    fn deleted_user_property_uses_serialized_fallback() {
        let dir = TestDir::new();
        dir.project();
        let mut source: Value =
            serde_json::from_slice(&fs::read(dir.0.join("scene.json")).unwrap()).unwrap();
        source["objects"][0]["visible"] =
            serde_json::json!({"user":"deleted_property", "value":false});
        dir.write("scene.json", source);
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert!(!scene.images[0].visible);
        assert!(scene.warnings.is_empty());
    }

    #[test]
    fn autosize_uses_texture_dimensions_unless_scene_supplies_size() {
        let dir = TestDir::new();
        dir.project();
        dir.write(
            "models/image.json",
            serde_json::json!({
                "autosize":true, "width":100, "height":200,
                "material":"materials/image.json"
            }),
        );
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert_eq!(scene.images[0].size, [1920., 1080.]);
        assert!(scene.warnings.is_empty());
        let mut source = scene.source;
        source["objects"][0].as_object_mut().unwrap().remove("size");
        dir.write("scene.json", source);
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert_eq!(scene.images[0].size, [0., 0.]);
        assert!(scene.warnings.is_empty());
    }

    #[test]
    fn supports_image_color_versions_and_rejects_unimplemented_shader_features() {
        let dir = TestDir::new();
        dir.project();
        for (combos, supported) in [
            (serde_json::json!({"VERSION":2}), true),
            (serde_json::json!({"VERSION":1,"LIGHTING":0}), true),
            (serde_json::json!({"VERSION":2,"LIGHTING":1}), false),
            (serde_json::json!({"VERSION":99}), false),
        ] {
            dir.write(
                "materials/image.json",
                serde_json::json!({"passes":[{
                    "shader":"genericimage2","textures":["example"],"combos":combos
                }]}),
            );
            let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
            assert_eq!(scene.warnings.is_empty(), supported);
        }
    }
    #[test]
    fn reports_unsupported_objects_and_effects() {
        let dir = TestDir::new();
        dir.project();
        let mut source: Value =
            serde_json::from_slice(&fs::read(dir.0.join("scene.json")).unwrap()).unwrap();
        source["objects"][0]["effects"] = serde_json::json!([{"file":"effects/water.json"}]);
        source["objects"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":8,"particle":"particles/snow.json"}));
        dir.write("scene.json", source);
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert!(scene.warnings.iter().any(|s| s.contains("effects")));
        assert!(scene.warnings.iter().any(|s| s.contains("particle")));
    }
    #[test]
    fn reports_ignored_camera_and_material_state() {
        let dir = TestDir::new();
        dir.project();
        let mut source: Value =
            serde_json::from_slice(&fs::read(dir.0.join("scene.json")).unwrap()).unwrap();
        source["camera"] = serde_json::json!({"center":"500 500 0","eye":"500 500 1","up":"0 1 0"});
        source["objects"][0]["colorBlendMode"] = serde_json::json!(2);
        source["objects"][0]["verticalalign"] = serde_json::json!("top");
        dir.write("scene.json", source);
        dir.write("materials/image.json",serde_json::json!({"passes":[{"shader":"genericimage2","textures":["example"],"usertextures":["mask"],"depthtest":"enabled"}]}));
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        for expected in [
            "colorBlendMode",
            "image alignment",
            "usertextures",
            "depthtest",
        ] {
            assert!(
                scene.warnings.iter().any(|s| s.contains(expected)),
                "{expected}"
            );
        }
    }
    #[test]
    fn accepts_standard_camera_and_decodes_image_colors() {
        let dir = TestDir::new();
        dir.project();
        let mut source: Value =
            serde_json::from_slice(&fs::read(dir.0.join("scene.json")).unwrap()).unwrap();
        source["camera"] = serde_json::json!({"center":"960 540 0","eye":"960 540 1","up":"0 1 0"});
        for encoded in ["255 128 0", "#ff8000", "#f80"] {
            source["objects"][0]["color"] = serde_json::json!(encoded);
            dir.write("scene.json", source.clone());
            let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
            assert!(scene.warnings.is_empty());
            assert_eq!(scene.images[0].color[0], 1.0);
            assert!(scene.images[0].color[1] > 0.5 && scene.images[0].color[1] < 0.54);
            assert_eq!(scene.images[0].color[2], 0.0);
        }
    }

    #[test]
    fn origin_camera_and_rotated_orthographic_views_project_canvas_points() {
        let dir = TestDir::new();
        dir.project();
        let mut source = Project::load(&dir.0, None).unwrap().scene().unwrap().source;
        source["camera"] = serde_json::json!({"center":"0 0 -1","eye":"0 0 0","up":"0 1 0"});
        dir.write("scene.json", source.clone());
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert!(scene.warnings.is_empty());
        assert_eq!(scene.camera.project([0., 0., 0.]), [0., 0.]);
        assert_eq!(scene.camera.project([1920., 1080., 0.]), [1920., 1080.]);
        source["camera"]["up"] = serde_json::json!("1 0 0");
        dir.write("scene.json", source);
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert!(scene.warnings.is_empty());
        assert_eq!(scene.camera.project([970., 540., 0.]), [960., 550.]);
    }
    #[test]
    fn camera_fade_is_inert_without_global_camera_paths() {
        let dir = TestDir::new();
        dir.project();
        let mut source = Project::load(&dir.0, None).unwrap().scene().unwrap().source;
        source["general"]["camerafade"] = true.into();
        dir.write("scene.json", source.clone());
        assert!(
            Project::load(&dir.0, None)
                .unwrap()
                .scene()
                .unwrap()
                .warnings
                .is_empty()
        );
        source["camera"] = serde_json::json!({"paths":["camera.json"]});
        dir.write("scene.json", source);
        assert!(
            Project::load(&dir.0, None)
                .unwrap()
                .scene()
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("global camera paths"))
        );
    }

    #[test]
    fn bloom_resolves_user_settings() {
        let dir = TestDir::new();
        dir.project();
        let mut source = Project::load(&dir.0, None).unwrap().scene().unwrap().source;
        source["general"]["bloom"] = true.into();
        source["general"]["bloomstrength"] = serde_json::json!({"user":"opacity","value":2});
        source["general"]["bloomthreshold"] = 0.3.into();
        dir.write("scene.json", source);
        let scene = Project::load(&dir.0, None).unwrap().scene().unwrap();
        assert!(scene.warnings.is_empty());
        let bloom = scene.bloom.unwrap();
        assert_eq!(bloom.strength, 0.25);
        assert_eq!(bloom.threshold, 0.3);
    }
    #[test]
    fn rejects_unknown_project_type_and_missing_entry() {
        let dir = TestDir::new();
        dir.write(
            "project.json",
            serde_json::json!({"type":"application","file":"app.exe"}),
        );
        assert!(
            Project::load(&dir.0, None)
                .unwrap_err()
                .to_string()
                .contains("unsupported wallpaper type")
        );
        dir.write(
            "project.json",
            serde_json::json!({"type":"video","file":"missing.mp4"}),
        );
        assert!(
            Project::load(&dir.0, None)
                .unwrap_err()
                .to_string()
                .contains("entry point")
        );
    }
}
