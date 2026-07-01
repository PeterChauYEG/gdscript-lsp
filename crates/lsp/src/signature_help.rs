use gdscript_api_db::ApiDb;
use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};

use crate::type_resolver::TypeMap;

/// Build a signature help response for a method call.
///
/// `receiver` is the object before `.`, `method_name` is the called method,
/// `active_param` is the zero-based argument index the cursor is in.
#[must_use]
pub fn signature_help_for_method(
    receiver: &str,
    method_name: &str,
    active_param: u32,
    type_map: &TypeMap,
    api_db: &ApiDb,
) -> Option<SignatureHelp> {
    let type_name = type_map.resolve(receiver)?;
    let chain = api_db.inheritance_chain(type_name);

    for class_name in &chain {
        let class = api_db.get_class(class_name)?;
        let method = class.methods.iter().find(|m| m.name == method_name)?;

        let params: Vec<ParameterInformation> = method
            .arguments
            .iter()
            .map(|a| {
                let label = if let Some(def) = &a.default_value {
                    format!("{}: {} = {}", a.name, a.type_name, def)
                } else {
                    format!("{}: {}", a.name, a.type_name)
                };
                ParameterInformation {
                    label: ParameterLabel::Simple(label),
                    documentation: None,
                }
            })
            .collect();

        let ret = method
            .return_value
            .as_ref()
            .map_or("void", |r| r.type_name.as_str());
        let args_str: Vec<String> = method
            .arguments
            .iter()
            .map(|a| format!("{}: {}", a.name, a.type_name))
            .collect();
        let label = format!(
            "{}.{}({}) -> {}",
            class_name,
            method.name,
            args_str.join(", "),
            ret
        );

        let sig = SignatureInformation {
            label,
            documentation: if method.description.is_empty() {
                None
            } else {
                Some(tower_lsp::lsp_types::Documentation::String(
                    method.description.clone(),
                ))
            },
            parameters: Some(params),
            active_parameter: Some(active_param),
        };

        return Some(SignatureHelp {
            signatures: vec![sig],
            active_signature: Some(0),
            active_parameter: Some(active_param),
        });
    }

    None
}
