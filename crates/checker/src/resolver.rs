use gdscript_core::types::ResolvedType;

/// Resolve a `GDScript` type annotation string to a `ResolvedType`.
#[must_use]
pub fn resolve_annotation(annotation: &str) -> ResolvedType {
    match annotation {
        "int" => ResolvedType::Primitive(gdscript_core::types::Primitive::Int),
        "float" => ResolvedType::Primitive(gdscript_core::types::Primitive::Float),
        "bool" => ResolvedType::Primitive(gdscript_core::types::Primitive::Bool),
        "String" => ResolvedType::Primitive(gdscript_core::types::Primitive::String),
        "void" => ResolvedType::Void,
        name => ResolvedType::Builtin(name.to_owned()),
    }
}
