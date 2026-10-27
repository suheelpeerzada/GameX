//! The fallback for games no other integration recognizes. It only uses the
//! engine-independent classifier, so any game can be opened and browsed.

use crate::model::{Capability, CapabilityStatus, GameObject};

use super::classify::Builder;
use super::tree::FileTree;
use super::{
    assets_capability, cap, characters_capability, movement_capability, visual_editing_from_maps, Context,
    Detection, EngineIntegration,
};

pub struct Generic;

impl EngineIntegration for Generic {
    fn id(&self) -> &'static str {
        "generic"
    }

    fn name(&self) -> &'static str {
        "Unknown engine"
    }

    fn detect(&self, _tree: &FileTree) -> Detection {
        Detection {
            score: 0.05,
            evidence: vec!["No engine markers were found, so files are sorted by type, name and folder.".into()],
        }
    }

    fn analyze(&self, tree: &FileTree, _ctx: &Context) -> Vec<GameObject> {
        Builder::new(tree).finish()
    }

    fn capabilities(&self, _tree: &FileTree, _ctx: &Context, objects: &[GameObject]) -> Vec<Capability> {
        let sources = objects.iter().filter(|o| o.kind == "source file").count();
        let compiled = objects
            .iter()
            .any(|o| matches!(o.kind.as_str(), "compiled code" | "native library" | "code library" | "executable"));
        let code = if sources > 0 {
            cap(
                "code",
                "Code",
                CapabilityStatus::Partial,
                format!("{sources} source or script files are readable and editable as text."),
            )
        } else if compiled {
            cap(
                "code",
                "Code",
                CapabilityStatus::Unsupported,
                "Only compiled code was found, and GameX has no decompiler for this kind of game yet.",
            )
        } else {
            cap("code", "Code", CapabilityStatus::Unknown, "No code was found.")
        };
        vec![
            code,
            characters_capability(objects),
            movement_capability(objects),
            assets_capability(objects),
            visual_editing_from_maps(
                objects,
                cap(
                    "visualEditing",
                    "Visual editing",
                    CapabilityStatus::Unknown,
                    "No level or scene files in a known format were found.",
                ),
            ),
            cap(
                "rebuilding",
                "Rebuilding",
                CapabilityStatus::Unknown,
                "GameX doesn't know how to repackage games of this kind yet.",
            ),
        ]
    }
}
