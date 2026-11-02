//! Structured-output schema support: filling a declared `schema`'s fields
//! with type-appropriate default values. This is what powers `generate
//! SchemaName` today (see `ailang-interpreter`'s handling of
//! `Expr::Generate`); a real model provider's `generate_structured` slots
//! in behind the same [`ModelProvider`](crate::ModelProvider) trait call
//! without any change to the language or this function's callers.

use ailang_ast::TypeExpr;
use ailang_runtime::Value;

/// The type-appropriate "empty" value for a schema field: `""` for
/// `string`, `0` for `int`, `0.0` for `float`, `false` for `bool`, `'\0'`
/// for `char`, `null` for `null`/`void`, an empty array/map for `[T]`/
/// `{K: V}`, and `Value::Null` for anything unresolvable (a named/function
/// type isn't meaningful as a schema field in v0.5).
pub fn default_value_for_type(ty: &TypeExpr) -> Value {
    match ty {
        TypeExpr::Int => Value::Int(0),
        TypeExpr::Float => Value::Float(0.0),
        TypeExpr::Bool => Value::Bool(false),
        TypeExpr::String => Value::string(""),
        TypeExpr::Char => Value::Char('\0'),
        TypeExpr::Null | TypeExpr::Void => Value::Null,
        TypeExpr::Array(_) => Value::array(vec![]),
        TypeExpr::Map(_, _) => Value::map(vec![]),
        TypeExpr::Function { .. } | TypeExpr::Named(_) => Value::Null,
    }
}

/// Builds a `Value::Map` from `{field_name: default_value}` for every
/// field in a schema, in declaration order.
pub fn fill_schema_defaults(fields: &[(String, TypeExpr)]) -> Value {
    Value::map(
        fields
            .iter()
            .map(|(name, ty)| (Value::string(name.clone()), default_value_for_type(ty)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_match_type() {
        assert_eq!(default_value_for_type(&TypeExpr::Int), Value::Int(0));
        assert_eq!(default_value_for_type(&TypeExpr::Float), Value::Float(0.0));
        assert_eq!(default_value_for_type(&TypeExpr::Bool), Value::Bool(false));
        assert_eq!(default_value_for_type(&TypeExpr::String), Value::string(""));
    }

    #[test]
    fn fills_schema_defaults_for_person_like_schema() {
        let fields = vec![
            ("name".to_string(), TypeExpr::String),
            ("age".to_string(), TypeExpr::Int),
            ("is_member".to_string(), TypeExpr::Bool),
        ];
        let value = fill_schema_defaults(&fields);
        if let Value::Map(m) = value {
            let entries = m.borrow();
            assert_eq!(entries.len(), 3);
            assert_eq!(entries[0], (Value::string("name"), Value::string("")));
            assert_eq!(entries[1], (Value::string("age"), Value::Int(0)));
            assert_eq!(entries[2], (Value::string("is_member"), Value::Bool(false)));
        } else {
            panic!("expected a Value::Map");
        }
    }
}
