//! ComfyUI Node IR — typed representation of `(comfy-node ...)` declarations.
//!
//! Domain types (ComfyIOType) are kept here, NOT in quarks, to avoid
//! polluting the global type system with ComfyUI-specific vocabulary.

use nc_parser::SexpNode;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ComfyIrError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ComfyIrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ComfyUI IR error in `{}`: {}", self.field, self.message)
    }
}

impl std::error::Error for ComfyIrError {}

fn err(field: &str, msg: &str) -> ComfyIrError {
    ComfyIrError { field: field.into(), message: msg.into() }
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, ComfyIrError> {
    match node {
        SexpNode::List(c) => Ok(c),
        SexpNode::Atom(s) => Err(err("node", &format!("expected list, got atom: {s}"))),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, ComfyIrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom, got list")),
    }
}

/// Strip surrounding quotes if present.
fn unquote(s: &str) -> &str {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// ComfyUI IO Types (domain-specific, NOT in quarks)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ComfyIOType {
    String, Image, Mask, Latent, Boolean, Int, Float, Combo,
    Conditioning, Sampler, Sigmas, Guider, Noise,
    Clip, ControlNet, Vae, Model, LoraModel,
    ClipVision, ClipVisionOutput, StyleModel, Gligen,
    UpscaleModel, Audio, Video,
    Webcam, Hooks, TimestepsRange, HookKeyframes,
    Floats, ModelPatch, LatentUpscaleModel,
    Any,
    /// Catch-all for domain-specific types not in the whitelist (e.g. POSE_KEYPOINT, AUDIO_RECORD).
    Custom(String),
}

impl ComfyIOType {
    pub fn from_str(s: &str) -> Result<Self, ComfyIrError> {
        match s {
            "STRING" => Ok(Self::String),
            "IMAGE" => Ok(Self::Image),
            "MASK" => Ok(Self::Mask),
            "LATENT" => Ok(Self::Latent),
            "BOOLEAN" => Ok(Self::Boolean),
            "INT" => Ok(Self::Int),
            "FLOAT" => Ok(Self::Float),
            "COMBO" => Ok(Self::Combo),
            "CONDITIONING" => Ok(Self::Conditioning),
            "SAMPLER" => Ok(Self::Sampler),
            "SIGMAS" => Ok(Self::Sigmas),
            "GUIDER" => Ok(Self::Guider),
            "NOISE" => Ok(Self::Noise),
            "CLIP" => Ok(Self::Clip),
            "CONTROL_NET" => Ok(Self::ControlNet),
            "VAE" => Ok(Self::Vae),
            "MODEL" => Ok(Self::Model),
            "LORA_MODEL" => Ok(Self::LoraModel),
            "CLIP_VISION" => Ok(Self::ClipVision),
            "CLIP_VISION_OUTPUT" => Ok(Self::ClipVisionOutput),
            "STYLE_MODEL" => Ok(Self::StyleModel),
            "GLIGEN" => Ok(Self::Gligen),
            "UPSCALE_MODEL" => Ok(Self::UpscaleModel),
            "AUDIO" => Ok(Self::Audio),
            "VIDEO" => Ok(Self::Video),
            "WEBCAM" => Ok(Self::Webcam),
            "HOOKS" => Ok(Self::Hooks),
            "TIMESTEPS_RANGE" => Ok(Self::TimestepsRange),
            "HOOK_KEYFRAMES" => Ok(Self::HookKeyframes),
            "FLOATS" => Ok(Self::Floats),
            "MODEL_PATCH" => Ok(Self::ModelPatch),
            "LATENT_UPSCALE_MODEL" => Ok(Self::LatentUpscaleModel),
            "*" => Ok(Self::Any),
            other => Ok(Self::Custom(other.to_string())),
        }
    }

    /// Python expression: `IO.STRING`, `IO.IMAGE`, etc.
    pub fn to_io_expr(&self) -> String {
        match self {
            Self::String => "IO.STRING".into(),
            Self::Image => "IO.IMAGE".into(),
            Self::Mask => "IO.MASK".into(),
            Self::Latent => "IO.LATENT".into(),
            Self::Boolean => "IO.BOOLEAN".into(),
            Self::Int => "IO.INT".into(),
            Self::Float => "IO.FLOAT".into(),
            Self::Combo => "IO.COMBO".into(),
            Self::Conditioning => "IO.CONDITIONING".into(),
            Self::Sampler => "IO.SAMPLER".into(),
            Self::Sigmas => "IO.SIGMAS".into(),
            Self::Guider => "IO.GUIDER".into(),
            Self::Noise => "IO.NOISE".into(),
            Self::Clip => "IO.CLIP".into(),
            Self::ControlNet => "IO.CONTROL_NET".into(),
            Self::Vae => "IO.VAE".into(),
            Self::Model => "IO.MODEL".into(),
            Self::LoraModel => "IO.LORA_MODEL".into(),
            Self::ClipVision => "IO.CLIP_VISION".into(),
            Self::ClipVisionOutput => "IO.CLIP_VISION_OUTPUT".into(),
            Self::StyleModel => "IO.STYLE_MODEL".into(),
            Self::Gligen => "IO.GLIGEN".into(),
            Self::UpscaleModel => "IO.UPSCALE_MODEL".into(),
            Self::Audio => "IO.AUDIO".into(),
            Self::Video => "IO.VIDEO".into(),
            Self::Webcam => "IO.WEBCAM".into(),
            Self::Hooks => "IO.HOOKS".into(),
            Self::TimestepsRange => "IO.TIMESTEPS_RANGE".into(),
            Self::HookKeyframes => "IO.HOOK_KEYFRAMES".into(),
            Self::Floats => "IO.FLOATS".into(),
            Self::ModelPatch => "IO.MODEL_PATCH".into(),
            Self::LatentUpscaleModel => "IO.LATENT_UPSCALE_MODEL".into(),
            Self::Any => "\"*\"".into(),
            Self::Custom(ref s) => format!("IO.{s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Input options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ComfyInputOptions {
    pub default: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub round: Option<f64>,
    pub multiline: bool,
    pub dynamic_prompts: bool,
    pub tooltip: Option<String>,
    pub label_on: Option<String>,
    pub label_off: Option<String>,
    pub control_after_generate: bool,
    pub options: Vec<String>,
    pub folder_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ComfyInputDecl {
    pub name: String,
    pub io_type: ComfyIOType,
    pub options: ComfyInputOptions,
}

#[derive(Debug, Clone)]
pub struct ComfyHiddenDecl {
    pub name: String,
    pub hidden_type: String,
}

#[derive(Debug, Clone)]
pub struct ComfyNodeDecl {
    pub class_name: String,
    pub category: String,
    pub description: Option<String>,
    pub display_name: Option<String>,
    pub function: String,
    pub output_node: bool,
    pub deprecated: bool,
    pub v3: bool,
    pub search_aliases: Vec<String>,
    pub required: Vec<ComfyInputDecl>,
    pub optional: Vec<ComfyInputDecl>,
    pub hidden: Vec<ComfyHiddenDecl>,
    pub return_types: Vec<ComfyIOType>,
    pub return_names: Vec<String>,
    pub output_tooltips: Vec<String>,
}

// ---------------------------------------------------------------------------
// S-expression parsing
// ---------------------------------------------------------------------------

impl ComfyNodeDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, ComfyIrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("comfy-node", "expected (comfy-node ClassName ...)"));
        }

        let class_name = expect_atom(&children[1])?.to_string();
        let category = node.keyword(":category")
            .map(|s| unquote(s).to_string())
            .ok_or_else(|| err("category", &format!("comfy-node {class_name} missing :category")))?;
        let function = node.keyword(":function")
            .map(|s| unquote(s).to_string())
            .ok_or_else(|| err("function", &format!("comfy-node {class_name} missing :function")))?;

        let description = node.keyword(":description").map(|s| unquote(s).to_string());
        let display_name = node.keyword(":display-name").map(|s| unquote(s).to_string());
        let output_node = node.keyword(":output-node").map(|s| s == "true").unwrap_or(false);
        let deprecated = node.keyword(":deprecated").map(|s| s == "true").unwrap_or(false);
        let v3 = node.keyword(":v3").map(|s| s == "true").unwrap_or(false);

        // Parse :search-aliases (list)
        let search_aliases = parse_keyword_list(node, ":search-aliases");

        // Parse sub-forms
        let mut required = vec![];
        let mut optional = vec![];
        let mut hidden = vec![];
        let mut return_types = vec![];
        let mut return_names = vec![];
        let mut output_tooltips = vec![];

        for child in children.iter().skip(2) {
            if let Some(tag) = child.tag() {
                match tag {
                    "required" => required = parse_inputs(child)?,
                    "optional" => optional = parse_inputs(child)?,
                    "hidden" => hidden = parse_hidden(child)?,
                    "returns" => return_types = parse_return_types(child)?,
                    "return-names" => return_names = collect_string_atoms(child),
                    "output-tooltips" => output_tooltips = collect_string_atoms(child),
                    _ => {}
                }
            }
        }

        Ok(ComfyNodeDecl {
            class_name, category, description, display_name, function,
            output_node, deprecated, v3, search_aliases,
            required, optional, hidden,
            return_types, return_names, output_tooltips,
        })
    }
}

/// Parse all `(comfy-node ...)` forms from a list of top-level S-expressions.
pub fn parse_comfy_nodes(nodes: &[SexpNode]) -> Result<Vec<ComfyNodeDecl>, ComfyIrError> {
    nodes.iter()
        .filter(|n| n.tag() == Some("comfy-node"))
        .map(ComfyNodeDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_keyword_list(node: &SexpNode, key: &str) -> Vec<String> {
    if let SexpNode::List(children) = node {
        // Find the keyword, then the next element should be a list
        for (i, child) in children.iter().enumerate() {
            if let SexpNode::Atom(s) = child {
                if s == key {
                    if let Some(SexpNode::List(items)) = children.get(i + 1) {
                        return items.iter()
                            .filter_map(|n| match n {
                                SexpNode::Atom(s) => Some(unquote(s).to_string()),
                                _ => None,
                            })
                            .collect();
                    }
                }
            }
        }
    }
    vec![]
}

fn collect_string_atoms(node: &SexpNode) -> Vec<String> {
    if let SexpNode::List(children) = node {
        children.iter().skip(1) // skip tag
            .filter_map(|n| match n {
                SexpNode::Atom(s) => Some(unquote(s).to_string()),
                _ => None,
            })
            .collect()
    } else {
        vec![]
    }
}

fn parse_return_types(node: &SexpNode) -> Result<Vec<ComfyIOType>, ComfyIrError> {
    if let SexpNode::List(children) = node {
        children.iter().skip(1) // skip "returns" tag
            .filter_map(|n| match n {
                SexpNode::Atom(s) => Some(ComfyIOType::from_str(s)),
                _ => None,
            })
            .collect()
    } else {
        Ok(vec![])
    }
}

fn parse_inputs(parent: &SexpNode) -> Result<Vec<ComfyInputDecl>, ComfyIrError> {
    let children = expect_list(parent)?;
    let mut inputs = vec![];

    for child in children.iter().skip(1) { // skip "required"/"optional" tag
        if let SexpNode::List(parts) = child {
            if parts.len() < 2 { continue; }

            let name = expect_atom(&parts[0])?.to_string();
            let type_str = expect_atom(&parts[1])?;
            let io_type = ComfyIOType::from_str(type_str)?;

            let mut options = ComfyInputOptions::default();
            let mut j = 2;
            while j < parts.len() {
                if let SexpNode::Atom(kw) = &parts[j] {
                    match kw.as_str() {
                        ":default" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.default = Some(unquote(v).to_string());
                            }
                        }
                        ":min" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.min = v.parse().ok();
                            }
                        }
                        ":max" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                if v.starts_with("0x") || v.starts_with("0X") {
                                    options.max = u64::from_str_radix(&v[2..], 16).ok().map(|n| n as f64);
                                } else {
                                    options.max = v.parse().ok();
                                }
                            }
                        }
                        ":step" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.step = v.parse().ok();
                            }
                        }
                        ":round" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.round = v.parse().ok();
                            }
                        }
                        ":multiline" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.multiline = v == "true";
                            }
                        }
                        ":dynamic-prompts" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.dynamic_prompts = v == "true";
                            }
                        }
                        ":tooltip" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.tooltip = Some(unquote(v).to_string());
                            }
                        }
                        ":label-on" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.label_on = Some(unquote(v).to_string());
                            }
                        }
                        ":label-off" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.label_off = Some(unquote(v).to_string());
                            }
                        }
                        ":control-after-generate" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.control_after_generate = v == "true";
                            }
                        }
                        ":folder-path" => {
                            j += 1;
                            if let Some(SexpNode::Atom(v)) = parts.get(j) {
                                options.folder_path = Some(unquote(v).to_string());
                            }
                        }
                        ":options" => {
                            j += 1;
                            if let Some(SexpNode::List(items)) = parts.get(j) {
                                options.options = items.iter()
                                    .filter_map(|n| match n {
                                        SexpNode::Atom(s) => Some(unquote(s).to_string()),
                                        _ => None,
                                    })
                                    .collect();
                            }
                        }
                        _ => {}
                    }
                }
                j += 1;
            }

            inputs.push(ComfyInputDecl { name, io_type, options });
        }
    }
    Ok(inputs)
}

fn parse_hidden(parent: &SexpNode) -> Result<Vec<ComfyHiddenDecl>, ComfyIrError> {
    let children = expect_list(parent)?;
    let mut hidden = vec![];
    for child in children.iter().skip(1) {
        if let SexpNode::List(parts) = child {
            if parts.len() >= 2 {
                let name = expect_atom(&parts[0])?.to_string();
                let hidden_type = expect_atom(&parts[1])?.to_string();
                hidden.push(ComfyHiddenDecl { name, hidden_type });
            }
        }
    }
    Ok(hidden)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(input: &str) -> ComfyNodeDecl {
        let sexp = SexpNode::parse_one(input).unwrap();
        ComfyNodeDecl::from_sexp(&sexp).unwrap()
    }

    #[test]
    fn parse_simple_node() {
        let decl = parse_one(
            r#"(comfy-node CLIPTextEncode
              :category "conditioning"
              :function "encode"
              :description "Encodes text using CLIP"
              :display-name "CLIP Text Encode (Prompt)"
              :search-aliases ("text" "prompt")
              (required
                (text STRING :multiline true :tooltip "The text")
                (clip CLIP))
              (returns CONDITIONING))"#,
        );

        assert_eq!(decl.class_name, "CLIPTextEncode");
        assert_eq!(decl.category, "conditioning");
        assert_eq!(decl.function, "encode");
        assert_eq!(decl.description.as_deref(), Some("Encodes text using CLIP"));
        assert_eq!(decl.display_name.as_deref(), Some("CLIP Text Encode (Prompt)"));
        assert_eq!(decl.search_aliases, vec!["text", "prompt"]);
        assert_eq!(decl.required.len(), 2);
        assert_eq!(decl.required[0].name, "text");
        assert_eq!(decl.required[0].io_type, ComfyIOType::String);
        assert!(decl.required[0].options.multiline);
        assert_eq!(decl.required[0].options.tooltip.as_deref(), Some("The text"));
        assert_eq!(decl.required[1].name, "clip");
        assert_eq!(decl.required[1].io_type, ComfyIOType::Clip);
        assert_eq!(decl.return_types, vec![ComfyIOType::Conditioning]);
    }

    #[test]
    fn parse_numeric_params() {
        let decl = parse_one(
            r#"(comfy-node EmptyLatentImage
              :category "latent"
              :function "generate"
              (required
                (width INT :default 512 :min 16 :max 16384 :step 8)
                (height INT :default 512 :min 16 :max 16384 :step 8)
                (batch_size INT :default 1 :min 1 :max 4096))
              (returns LATENT))"#,
        );

        assert_eq!(decl.required.len(), 3);
        assert_eq!(decl.required[0].options.default.as_deref(), Some("512"));
        assert_eq!(decl.required[0].options.min, Some(16.0));
        assert_eq!(decl.required[0].options.max, Some(16384.0));
        assert_eq!(decl.required[0].options.step, Some(8.0));
        assert_eq!(decl.return_types, vec![ComfyIOType::Latent]);
    }

    #[test]
    fn parse_combo_options() {
        let decl = parse_one(
            r#"(comfy-node ImageScale
              :category "image/upscaling"
              :function "upscale"
              (required
                (image IMAGE)
                (upscale_method COMBO :options ("nearest-exact" "bilinear" "area"))
                (crop COMBO :options ("disabled" "center")))
              (returns IMAGE))"#,
        );

        assert_eq!(decl.required[1].io_type, ComfyIOType::Combo);
        assert_eq!(decl.required[1].options.options, vec!["nearest-exact", "bilinear", "area"]);
    }

    #[test]
    fn parse_folder_path_combo() {
        let decl = parse_one(
            r#"(comfy-node CheckpointLoaderSimple
              :category "loaders"
              :function "load_checkpoint"
              (required
                (ckpt_name COMBO :folder-path "checkpoints"))
              (returns MODEL CLIP VAE))"#,
        );

        assert_eq!(decl.required[0].options.folder_path.as_deref(), Some("checkpoints"));
        assert_eq!(decl.return_types.len(), 3);
        assert_eq!(decl.return_types[0], ComfyIOType::Model);
        assert_eq!(decl.return_types[1], ComfyIOType::Clip);
        assert_eq!(decl.return_types[2], ComfyIOType::Vae);
    }

    #[test]
    fn parse_output_node_with_hidden() {
        let decl = parse_one(
            r#"(comfy-node SaveImage
              :category "image"
              :function "save_images"
              :output-node true
              (required
                (images IMAGE)
                (filename_prefix STRING :default "ComfyUI"))
              (hidden
                (prompt PROMPT)
                (extra_pnginfo EXTRA_PNGINFO))
              (returns))"#,
        );

        assert!(decl.output_node);
        assert_eq!(decl.hidden.len(), 2);
        assert_eq!(decl.hidden[0].name, "prompt");
        assert_eq!(decl.hidden[0].hidden_type, "PROMPT");
        assert!(decl.return_types.is_empty());
    }

    #[test]
    fn io_type_roundtrip() {
        assert_eq!(ComfyIOType::String.to_io_expr(), "IO.STRING");
        assert_eq!(ComfyIOType::Conditioning.to_io_expr(), "IO.CONDITIONING");
        assert_eq!(ComfyIOType::Vae.to_io_expr(), "IO.VAE");
        assert_eq!(ComfyIOType::Model.to_io_expr(), "IO.MODEL");
        assert_eq!(ComfyIOType::LoraModel.to_io_expr(), "IO.LORA_MODEL");
    }
}
