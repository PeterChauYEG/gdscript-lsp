#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Primitive {
    Int,
    Float,
    Bool,
    String,
    NodePath,
    StringName,
    Rid,
    Callable,
    Signal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedType {
    Builtin(String),
    UserClass(String),
    Primitive(Primitive),
    Void,
    Unknown,
}
