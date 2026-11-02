//! Prompt template rendering: `${name}` interpolation against a lookup
//! function, plus template reuse (the same [`PromptValue`](ailang_runtime::PromptValue)
//! can be rendered many times against different variable bindings).

/// Renders `template`, replacing every `${name}` with the result of
/// `lookup(name)` (or an empty string if `lookup` returns `None` — an
/// undefined interpolation target is a compile-time *warning* from
/// `ailang-semantic`, see that crate's `check_prompt_interpolation`, not a
/// render-time hard error, so a prompt can still render partially).
///
/// Kept generic over the lookup function (rather than taking an
/// `Environment` directly) so this module has no dependency on the
/// interpreter's environment type, keeping `ailang-ai-runtime`'s
/// dependency graph one-directional (see the crate-level doc).
pub fn render_prompt(template: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = template[i + 2..].find('}') {
                let name = &template[i + 2..i + 2 + end];
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    out.push_str(&lookup(name).unwrap_or_default());
                    i += 2 + end + 1;
                    continue;
                }
            }
        }
        out.push(template[i..].chars().next().unwrap());
        i += template[i..].chars().next().unwrap().len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup_from<'a>(map: &'a HashMap<&str, &str>) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| map.get(name).map(|s| s.to_string())
    }

    #[test]
    fn interpolates_a_single_variable() {
        let mut vars = HashMap::new();
        vars.insert("topic", "quantum computing");
        let out = render_prompt("Research ${topic} thoroughly.", lookup_from(&vars));
        assert_eq!(out, "Research quantum computing thoroughly.");
    }

    #[test]
    fn interpolates_multiple_variables() {
        let mut vars = HashMap::new();
        vars.insert("name", "Alice");
        vars.insert("role", "researcher");
        let out = render_prompt("${name} is a ${role}.", lookup_from(&vars));
        assert_eq!(out, "Alice is a researcher.");
    }

    #[test]
    fn undefined_interpolation_renders_as_empty_string() {
        let vars = HashMap::new();
        let out = render_prompt("Hello ${missing}!", lookup_from(&vars));
        assert_eq!(out, "Hello !");
    }

    #[test]
    fn leaves_text_without_interpolation_markers_untouched() {
        let vars = HashMap::new();
        let out = render_prompt("You are an expert researcher.", lookup_from(&vars));
        assert_eq!(out, "You are an expert researcher.");
    }

    #[test]
    fn handles_multibyte_characters_around_interpolation() {
        let mut vars = HashMap::new();
        vars.insert("name", "世界");
        let out = render_prompt("こんにちは ${name}!", lookup_from(&vars));
        assert_eq!(out, "こんにちは 世界!");
    }
}
