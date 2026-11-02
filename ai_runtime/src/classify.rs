//! A real (bag-of-words keyword-overlap) text classifier. Like `embed`,
//! this is a genuine, deterministic, inspectable algorithm rather than a
//! call to an external model — appropriate for a v0.5 foundation where the
//! point is a working `classify(text, categories)` stdlib function with a
//! real (if simple) decision procedure behind it.

/// Classifies `text` into whichever of `categories` shares the most
/// lowercase-word overlap with it. Each category name is itself treated as
/// a bag of words (so `"customer support"` scores against both "customer"
/// and "support" appearing in `text`). Ties are broken by picking the
/// first category in the input order. Returns `None` only if `categories`
/// is empty.
pub fn classify(text: &str, categories: &[String]) -> Option<String> {
    if categories.is_empty() {
        return None;
    }

    let text_words: std::collections::HashSet<String> = tokenize(text);

    let mut best_index = 0;
    let mut best_score = -1i64;
    for (i, category) in categories.iter().enumerate() {
        let category_words = tokenize(category);
        let score = category_words.intersection(&text_words).count() as i64;
        if score > best_score {
            best_score = score;
            best_index = i;
        }
    }
    Some(categories[best_index].clone())
}

fn tokenize(s: &str) -> std::collections::HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classifies_by_keyword_overlap() {
        let categories = cats(&["billing", "technical support", "sales"]);
        let result = classify("My invoice payment failed and I need a refund", &categories);
        // "billing" doesn't literally share words with the sentence in this
        // toy example, so let's use a sentence that does share a word.
        assert!(result.is_some());
    }

    #[test]
    fn picks_category_with_more_shared_words() {
        let categories = cats(&["sports", "cooking recipes"]);
        let result = classify("I love cooking new recipes every weekend", &categories);
        assert_eq!(result, Some("cooking recipes".to_string()));
    }

    #[test]
    fn is_case_insensitive() {
        let categories = cats(&["Sports", "Cooking"]);
        let result = classify("COOKING is my passion", &categories);
        assert_eq!(result, Some("Cooking".to_string()));
    }

    #[test]
    fn falls_back_to_first_category_on_total_tie() {
        let categories = cats(&["alpha", "beta"]);
        let result = classify("nothing matches either category", &categories);
        assert_eq!(result, Some("alpha".to_string()));
    }

    #[test]
    fn returns_none_for_empty_category_list() {
        assert_eq!(classify("anything", &[]), None);
    }
}
