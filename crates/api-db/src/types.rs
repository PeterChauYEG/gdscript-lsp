use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ClassDef {
    pub name: String,
    pub is_refcounted: bool,
    pub is_instantiable: bool,
    pub inherits: Option<String>,
    pub api_type: String,
    #[serde(default)]
    pub methods: Vec<MethodDef>,
    #[serde(default)]
    pub properties: Vec<PropertyDef>,
    #[serde(default)]
    pub signals: Vec<SignalDef>,
    #[serde(default)]
    pub constants: Vec<ConstantDef>,
    #[serde(default)]
    pub enums: Vec<EnumDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MethodDef {
    pub name: String,
    pub is_const: bool,
    pub is_static: bool,
    pub is_vararg: bool,
    #[serde(default)]
    pub arguments: Vec<ArgumentDef>,
    pub return_value: Option<ReturnValue>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArgumentDef {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReturnValue {
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyDef {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub setter: Option<String>,
    pub getter: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignalDef {
    pub name: String,
    #[serde(default)]
    pub arguments: Vec<ArgumentDef>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConstantDef {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub values: Vec<EnumValueDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnumValueDef {
    pub name: String,
    pub value: i64,
}
