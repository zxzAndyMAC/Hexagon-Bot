//! ADR 0079: the host generates the scope; the owner chooses once or project.
//! Generalize only bounded path families. Unknown shell semantics stay exact;
//! a false negative costs another question, a false positive an unreviewed effect.
use serde_json::Value;

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct PermissionShapeSuggestion {
    pub shape: String,
    pub generalized: bool,
    pub tool: String,
    pub project_id: String,
    pub agent_id: String,
}

pub(crate) fn suggest(tool: &str, input: &Value) -> Option<(String, bool)> {
    if crate::permissions::is_safety_net(tool, input).is_some() || tool.starts_with("mcp:") {
        return None;
    }
    let target = match tool {
        "bash" => input["cmd"].as_str()?,
        "web_fetch" => input["url"].as_str()?,
        "fs_read" | "fs_write" | "fs_patch" | "artifact_read" | "artifact_write" => {
            input["path"].as_str()?
        }
        _ => return None,
    };
    if target.is_empty() {
        return None;
    }
    let candidate = match tool {
        "web_fetch" => {
            let url = url::Url::parse(target).ok()?;
            if !matches!(url.scheme(), "https" | "http")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return None;
            }
            // A directory family preserves scheme, authority, port and parent
            // path. Query-bearing calls stay exact (queries may carry actions
            // or credentials); never replace a whole origin with a wildcard.
            let (parent, _) = url.path().rsplit_once('/')?;
            if url.query().is_none()
                && url.fragment().is_none()
                && !parent.is_empty()
                && !parent.contains(['*', '?', '%'])
            {
                Some(format!("{}{parent}/*", url.origin().ascii_serialization()))
            } else {
                None
            }
        }
        "bash" => None,
        _ => {
            let path = std::path::Path::new(target);
            if crate::tools::sensitive_file_path(path)
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return None;
            }
            None
        }
    };
    if let Some(shape) = candidate {
        if crate::permissions::shape_matches(&shape, tool, input) {
            return Some((shape, true));
        }
    }
    let exact = format!("exact:{target}");
    crate::permissions::shape_matches(&exact, tool, input).then_some((exact, false))
}
